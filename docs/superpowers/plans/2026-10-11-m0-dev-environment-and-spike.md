# M0: Dev Environment + Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Fenji the same working repo, tooling and process Caphras had. Record the pinned SoLO build. Write up the static triage. Make the sandbox able to run the client non-root. Observe the real client once inside the sandbox, against our stub. Release 0.1.0 with the owner's go-ahead.

**Architecture:** Most of M0 is a port of the halted Caphras repo (`~/projects/caphras`) with names changed: hooks, checks, bootstrap, Claude Code configuration, docs, templates, CI, the `stub` crate and the bubblewrap sandbox. Two things are new: game-specific content (ground rules, forbidden file types, labels and milestones, RE docs), and a `--no-root` sandbox mode, needed because umu refuses to run as uid 0. The spike rests on the 2026-10-11 static triage (raw outputs in `~/projects/caphras/re/solo/`) plus one isolated runtime observation.

**Tech Stack:** Rust 1.97.1 (edition 2024), tokio 1.53, clap 4.6, tracing 0.1, tempfile 3.27. bash (macOS 3.2-compatible except the Linux-only sandbox), bubblewrap, python3 (stdlib). GitHub Actions. REA 6.3.0 and Ghidra 12.1.4. umu-launcher 1.4.4 with GE-Proton.

**Spec:** `docs/superpowers/specs/2026-10-11-fenji-design.md`. Read §2 (ground rules), §4 (discovery), §5.4 (kernel driver gate), §10 (milestones) and §13 (lineage) before starting.

## Global Constraints

- Legitimate copies only. Never distribute the client, point to unofficial copies or support cracked clients (spec §2.1).
- Clean room. No leaked server material, nor anything derived from it.
  - The RaGEZONE "GuJian Online server RE report" is rejected.
  - Anyone who has read rejected material does no RE or protocol work on the client (spec §2.2, CLAUDE.md "Clean-room hygiene").
- Nothing from the game in git (spec §2.3): binaries, data archives, extracted assets, Lua bytecode or decompiled scripts, Oodle or other middleware DLLs, captures, stub logs, REA bundles. They live in `$FENJI_CLIENT_PATH` and the git-ignored `re/`.
- Every run of the client happens inside `scripts/spike/isolated-run.sh`, against our own stub (spec §2.4).
- Decision gates stop work: client patching, circumventing protection, anything that would load the embedded AGT kernel driver, Oodle outside its licence (spec §2.6). Open a `Decision gate` issue and wait.
- Our code never handles Steam or Gameforge credentials (spec §2.7).
- Rust 1.97.1 via `rust-toolchain.toml`; one workspace version in `[workspace.package]` (`0.0.0` until the M0 release); `publish = false`; `unsafe_code = "forbid"`.
- Conventional Commits 1.0.0, with scopes `proto`, `data`, `lua`, `auth`, `store`, `server`, `launcher`, `tools`, `content`, `deploy`, `docs`, `re`, `deps`, `release`. One PR per concern into `dev`. Each task from Task 3 on is one issue → one branch → one PR (`Closes #N`).
- Port source: the Caphras repo at `~/projects/caphras`, ref `dev` (`ce27922`). The exception is the sandbox, which comes from `1e23272` (Caphras PR #25, reviewed). Rename rule for ported files: `caphras`→`fenji`, `Caphras`→`Fenji`, `CAPHRAS`→`FENJI`. BDO-specific text is replaced exactly as each task says.
- The owner's standing instruction: squash-merge PRs into `dev` once CI is green and the task review is clean. Merging into `main`, tagging and releasing need the owner's go-ahead.
- M0 releases as `0.1.0` (spec §10).

## Review Focus

1. **A leftover Caphras path** (`~/.config/caphras`, `~/.local/share/caphras`, `CAPHRAS_CLIENT_PATH`) would silently point the new repo at the halted project's tools and environment. Covered by Task 1 Step 9 (repo-wide grep with an allow-list).
2. **The forbidden-file check missing SoLO artefacts:** `data0NN` and `_index` archives, Lua bytecode, archive blocks, Oodle DLLs, extracted tables. Covered by Task 1's new `scripts/test-check-forbidden-files.sh`.
3. **`--no-root` quietly keeping a capability or losing an isolation property.** Covered by Task 4's self-test checks.
4. **A pinned copy made inside a Steam library, or carrying the owner's run-time artefacts as if they were the build.** Covered by Task 5 Steps 2–4.
5. **A client run reaching anything but loopback,** including Gameforge, Steam or Epic hosts. Covered by Task 7 Steps 1 and 6: the sandbox self-test passes before the run, and every connect in the run log is to loopback or failed.

## File map

| Path | Task | Responsibility |
|---|---|---|
| `.gitignore`, `.githooks/*`, `.shellcheckrc` | 1 | Repo guards |
| `scripts/check-forbidden-files.sh`, `scripts/test-check-forbidden-files.sh` | 1 | Keep game content out (SoLO file types), and its tests |
| `scripts/check-commit-subject.sh`, `scripts/test-check-commit-subject.sh` | 1 | Conventional Commit subjects (hook + CI) |
| `scripts/versions.env`, `scripts/bootstrap.sh`, `scripts/rea-mcp.sh`, `scripts/ghidra-mcp-bridge.constraints.txt`, `rust-toolchain.toml` | 1 | Pinned dev environment |
| `CLAUDE.md`, `.claude/settings.json`, `.mcp.json` | 1 | Working rules and Claude Code configuration |
| `docs/README.md`, `docs/development.md`, `docs/re/README.md`, `docs/re/sources.md`, `docs/protocol/README.md` | 1 | How we work; RE workflow; sources (used and rejected) |
| `.github/ISSUE_TEMPLATE/*`, `.github/pull_request_template.md`, `scripts/github-setup.sh`, `cliff.toml` | 1 | Issues, PRs, labels/milestones, changelog |
| `Cargo.toml`, `Cargo.lock`, `deny.toml`, `tools/stub/` | 1 | Workspace and the `fenji-stub` listener |
| `.github/workflows/ci.yml` | 1, 3 | CI jobs |
| `CHANGELOG.md` | 1, 8 | Generated by git-cliff |
| `scripts/spike/isolated-run.sh`, `scripts/spike/test-isolated-run.sh`, `scripts/spike/seccomp-deny-keys.py` | 3, 4 | Sandbox and its self-test |
| `docs/re/pinned-build.md` | 5 | The exact build we target |
| `docs/re/spike-0.md` | 6, 7, 8 | Spike findings (prose + evidence only) |
| `docs/superpowers/specs/2026-10-11-fenji-design.md` | 8 | M0 answers to the open questions |

---

### Task 1: Port the repo foundation (local, before the remote)

Delivered as a sequence of commits directly on local `dev` (there is no remote yet, so no PR). It's reviewed as one unit.

**Files:** everything in the file map marked Task 1.

**Interfaces:**
- Consumes: the Caphras repo at `~/projects/caphras`, ref `dev`.
- Produces, for later tasks:
  - `scripts/check-forbidden-files.sh [--staged]` (exit 1 on a forbidden file);
  - `fenji-stub` (crate `tools/stub`, lib `fenji_stub`, binary `fenji-stub --listen ADDR... --out DIR`);
  - the CI jobs `rust`, `deny`, `repo-hygiene` and `pr-title`;
  - `scripts/github-setup.sh [--dry-run]`.

- [ ] **Step 1: Copy the files that only need renaming** (from `~/projects/fenji`; `git archive` keeps executable bits):

```bash
cd ~/projects/fenji
paths=(.githooks .shellcheckrc scripts/check-commit-subject.sh scripts/test-check-commit-subject.sh
  scripts/versions.env scripts/bootstrap.sh scripts/rea-mcp.sh scripts/ghidra-mcp-bridge.constraints.txt
  rust-toolchain.toml cliff.toml deny.toml Cargo.toml Cargo.lock tools/stub
  .claude/settings.json .mcp.json
  .github/ISSUE_TEMPLATE/config.yml .github/ISSUE_TEMPLATE/task.yml .github/ISSUE_TEMPLATE/decision-gate.yml
  .github/ISSUE_TEMPLATE/bug.yml .github/ISSUE_TEMPLATE/re-investigation.yml .github/pull_request_template.md
  .github/workflows/ci.yml docs/development.md CLAUDE.md)
git -C ~/projects/caphras archive --format=tar dev -- "${paths[@]}" | tar -x
# Rename only inside the ported paths: the spec and this plan mention Caphras on purpose (lineage).
grep -rlI -e caphras -e Caphras -e CAPHRAS -- "${paths[@]}" \
  | xargs -r sed -i -e 's/caphras/fenji/g' -e 's/Caphras/Fenji/g' -e 's/CAPHRAS/FENJI/g'
```

`CLAUDE.md` is rewritten in Step 5. It is copied here only so its last three sections can be kept.

- [ ] **Step 2: Write the failing test for the SoLO forbidden-file rules**: create `scripts/test-check-forbidden-files.sh` (mode 755):

```bash
#!/usr/bin/env bash
# Tests for scripts/check-forbidden-files.sh. Runs anywhere bash runs (macOS bash 3.2, Fedora, CI).
set -euo pipefail

check="$(cd "$(dirname "$0")" && pwd)/check-forbidden-files.sh"
passed=0
ok()   { echo "ok - $1"; passed=$((passed + 1)); }
fail() { echo "FAIL - $1" >&2; exit 1; }

root=""
trap 'rm -rf "$root"' EXIT
root=$(mktemp -d)

# staged PATH FORMAT: a fresh repo with only PATH staged (content from printf FORMAT); prints its dir.
staged() {
  local d
  d=$(mktemp -d "$root/r.XXXXXX")
  git -C "$d" init -q
  mkdir -p "$d/$(dirname "$1")"
  # shellcheck disable=SC2059 # the format carries octal escapes on purpose
  printf "$2" > "$d/$1"
  git -C "$d" add -f -- "$1"
  echo "$d"
}

# rejected PATH FORMAT WHAT: the staged file must fail the check.
rejected() {
  local d
  d=$(staged "$1" "$2")
  if (cd "$d" && "$check" --staged) >/dev/null 2>&1; then fail "$3 ($1) was accepted"; fi
  ok "rejects $3 ($1)"
}

d=$(staged docs/notes.md '# notes\n')
(cd "$d" && "$check" --staged) >/dev/null 2>&1 || fail "an ordinary markdown file was rejected"
ok "accepts an ordinary file"

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

d=$(staged big.txt 'x')
head -c $((5 * 1024 * 1024 + 1)) /dev/zero > "$d/big.txt"
git -C "$d" add -f big.txt
if (cd "$d" && "$check" --staged) >/dev/null 2>&1; then fail "a file over 5 MiB was accepted"; fi
ok "rejects a file over 5 MiB"

echo "all $passed checks passed"
```

- [ ] **Step 3: Run it to see it fail**: `bash scripts/test-check-forbidden-files.sh`.
Expected: FAIL. Either the checker is missing (it wasn't copied in Step 1), or the copied Caphras checker accepts a SoLO case such as `data/data042`.

- [ ] **Step 4: Write `scripts/check-forbidden-files.sh`** (mode 755) and `.gitignore`:

```bash
#!/usr/bin/env bash
# Rejects game content and RE artefacts from the repo (design spec §2 rule 3, §9).
#
# Usage:
#   scripts/check-forbidden-files.sh            # check all tracked files (CI)
#   scripts/check-forbidden-files.sh --staged   # check staged files (pre-commit hook)
#
# Compatible with macOS's bash 3.2.
set -euo pipefail

MAX_BYTES=$((5 * 1024 * 1024))

if [ "${1:-}" = "--staged" ]; then
  files=$(git diff --cached --name-only --diff-filter=ACMR)
else
  files=$(git ls-files)
fi

failed=0
fail() {
  echo "forbidden: $1 ($2)" >&2
  failed=1
}

# Reads the first 8 bytes of a file as lowercase hex.
magic() {
  head -c 8 "$1" 2>/dev/null | od -An -tx1 | tr -d ' \n'
}

while IFS= read -r f; do
  [ -z "$f" ] && continue
  [ -f "$f" ] || continue

  case "$f" in
    re/*) fail "$f" "local RE workspace"; continue ;;
  esac

  lower=$(printf '%s' "$f" | tr '[:upper:]' '[:lower:]')
  # Digits spelled out: a range like [0-9] can match other characters in some locales.
  case "${lower##*/}" in
    data[0123456789][0123456789][0123456789]|_index)
      fail "$f" "game data archive"; continue ;;
  esac
  case "$lower" in
    *.exe|*.dll|*.sys|*.pae|*.vplugin|*.luac|*.tab|*.bk2|*.bank|*.fsb|*.pcap|*.pcapng|*.gpr|*.hop|*.i64|*.idb)
      fail "$f" "forbidden file type"; continue ;;
    *.rep/*)
      fail "$f" "Ghidra project data"; continue ;;
  esac

  m=$(magic "$f")
  case "$m" in
    4d5a*) fail "$f" "Windows PE binary (MZ header)" ;;
    1b4c7561*) fail "$f" "Lua bytecode" ;;
    5a465300*) fail "$f" "game archive block (ZFS)" ;;
    d4c3b2a1*|a1b2c3d4*|4d3cb2a1*|a1b23c4d*) fail "$f" "pcap capture" ;;
    0a0d0d0a*) fail "$f" "pcapng capture" ;;
  esac

  size=$(wc -c < "$f" | tr -d ' ')
  if [ "$size" -gt "$MAX_BYTES" ]; then
    fail "$f" "larger than 5 MiB; game data must never be committed"
  fi
done <<EOF
$files
EOF

if [ "$failed" -ne 0 ]; then
  echo "Game binaries, archives, scripts, captures and RE artefacts must stay out of the repo (see docs/re/README.md)." >&2
  exit 1
fi
```

`.gitignore`:

```
# Rust
/target/
**/*.rs.bk

# Local reverse-engineering workspace: Ghidra/REA sessions, evidence bundles, decompiled output,
# extracted scripts, sandbox runs and stub logs. NEVER commit.
/re/

# Game content must never enter the repo (ground rule: no redistribution)
*.exe
*.dll
*.sys
*.pae
*.vplugin
*.vPlugin
*.luac
data[0-9][0-9][0-9]
_index
*.tab
*.bk2
*.bank
*.fsb
*.pcap
*.pcapng
*.gpr
*.rep/
*.hop
*.i64
*.idb

# Local server state and secrets
*.sqlite
*.sqlite-wal
*.sqlite-shm
*.db
secrets.env
.env
fenji.local.toml

# Editors / OS
.DS_Store
.idea/
.vscode/
```

Run: `bash scripts/test-check-forbidden-files.sh`. Expected: 18 `ok -` lines and `all 18 checks passed`.

- [ ] **Step 5: Write `CLAUDE.md`.** Replace the copied file. The new text is the block below, followed by three sections taken from the copied (already renamed) Caphras file, unchanged: `## Pinned software`, `## Claude Code plugins, skills and MCP servers` and `## Machines`. The `## Machines` paragraph then becomes the one at the end of the block.

```markdown
# Fenji

Clean-room private server emulator for Swords of Legends Online (SoLO: the Western build of 古剑奇谭网络版, Steam app 1418100; servers shut down 2023-06-30), in Rust, for a small invited group on the owner's VPS. Built by reverse engineering the owner's own licensed client. Design: `docs/superpowers/specs/2026-10-11-fenji-design.md`. Docs index: `docs/README.md`. Fenji replaces the halted Caphras project (Black Desert Online) and keeps its tooling and process.

**Status:** M0 (dev environment + spike) in progress: `docs/superpowers/plans/2026-10-11-m0-dev-environment-and-spike.md`.

## Ground rules (never break these)

1. **Legitimate copies only.** Every player and the host needs their own legitimately obtained SoLO client: Steam (app 1418100 already in their library; it's delisted) or Gameforge's own launcher from when it was distributed. Never distribute the client, point to unofficial copies, or support cracked or modified clients. There's no automated ownership check; the owner vets everyone they invite.
2. **Clean room.** Never use, reference or accept leaked SoLO / 古剑奇谭网络版 server files, binaries, scripts or source, or anything derived from them. That includes the GuJian Online "server RE report" from RaGEZONE (rejected; see `docs/re/sources.md`) and anything from the unofficial servers. Community material only if clearly licensed with clean provenance, as hints or external tools, listed in `docs/re/sources.md`.
3. **No game content in the repo.** No binaries, data archives, extracted assets, extracted or decompiled Lua, middleware libraries (Oodle, …), REA evidence bundles, captures or stub logs. These live in `$FENJI_CLIENT_PATH` and the git-ignored `re/`. `scripts/check-forbidden-files.sh` enforces this; never bypass it.
4. **Never connect a modified or instrumented client to a live official service.** The game still runs in China, and Gameforge, Steam and Epic services are live. Every client run happens inside `scripts/spike/isolated-run.sh`, against our own stub or server.
5. **Private, invite-only, non-commercial.**
6. **Decision gates stop work:** any client patching (on disk or in memory), circumventing any protection, anything that would load the client's embedded AGT kernel driver, and using proprietary middleware (Oodle) outside its licence. Report findings and wait for the owner's decision.
7. Our code never handles Steam or Gameforge credentials; players type them into DepotDownloader or the services' own pages. Fenji's own account passwords are stored only as argon2id hashes.

## How to work here

- **Process (superpowers plugin):** `brainstorming` → spec → `writing-plans` → `subagent-driven-development` or `executing-plans`; `test-driven-development` for code; `systematic-debugging` for bugs; `verification-before-completion` before claiming done. One implementation plan per milestone in `docs/superpowers/plans/`.
- **Reverse engineering:** use the `reverse-engineer-anything` skill, the shared Ghidra GUI project and the `rea` MCP server. Static analysis first. The Ghidra project is the main surface for `gujianol.exe`, because REA's full auto-analysis of its ~75,000 functions is too slow. REA records Evidence IDs for targeted functions. Cite REA Evidence IDs (or pinned-exe addresses where REA can't reach) in `docs/protocol/` and in packet type doc comments. Separate observation, inference and unknown. Close REA sessions when done.
- **Clean-room hygiene:** anyone, person or agent, who has read material listed as rejected in `docs/re/sources.md` does no RE or protocol work on the client. They may coordinate, review process and write code that doesn't depend on client internals.
- **Ghidra GUI project (`ghidra` MCP):** canonical names, types and structs for the pinned binary, shared with the owner live. Leave `GHIDRA_MCP_ALLOW_SCRIPTS` unset (script execution off) and the server on 127.0.0.1. Never use its `debugger_*` tools on the SoLO client: attaching a debugger is a decision gate.
- **Client runs:** only inside `scripts/spike/isolated-run.sh` (loopback only; a nested display with `--x11 N`; non-root with `--no-root`). Quit Steam first, bind only directories that belong to the run, and keep the pinned copy outside any Steam library.
- **Launcher UI** (`apps/launcher`, Tauri 2): use the `impeccable` skill for design work.
- **Library/API docs:** use `context7` rather than memory for crate and tool APIs (Tauri, Bevy, sqlx, axum, binrw).
- **Live server inspection:** `brp` MCP against a local `fenji-server` (BRP binds to localhost only).
- Push work branches and squash-merge PRs into `dev` yourself once CI is green and the task review is clean (owner's standing instruction, 2026-10-11); rebase stacked children onto `dev` after each parent lands. Merging into `main`, tagging and releasing still need the owner's go-ahead.

## Git, versions, changelog (full rules: `docs/development.md`)

- `dev` = default and integration branch. `main` = tagged releases only. Cut work branches from `dev`: `feat/<scope>-<topic>`, `fix/…`, `docs/…`, `chore/…`. Releases via `release/vX.Y.Z`; urgent fixes via `hotfix/vX.Y.Z` from `main`.
- **Conventional Commits 1.0.0**, enforced by `.githooks/commit-msg`. Scopes: `proto`, `data`, `lua`, `auth`, `store`, `server`, `launcher`, `tools`, `content`, `deploy`, `docs`, `re`, `deps`, `release`.
- **SemVer 2.0.0**, one workspace version. Milestone releases bump minor (M0 → 0.1.0 … M4 → 0.5.0); M5 → 1.0.0. Fixes bump patch.
- `CHANGELOG.md` (Keep a Changelog 1.1.0) is generated by `git-cliff` from commits. Never hand-edit it.
- **Issues track all work:** milestones M0–M5 on GitHub; each approved plan task becomes a `Task` issue. RE questions → `RE investigation` issues; gates → `Decision gate` issues (work stops until the owner fills in the decision).
- **One PR per concern**, linked with `Closes #N`, squash-merged into `dev`. Branch names carry the issue number: `feat/proto-12-handshake-state-machine`.

## Commands

```bash
scripts/bootstrap.sh                # set up / update this machine (macOS or Linux)
scripts/bootstrap.sh --check        # installed vs pinned versions + REA readiness
scripts/check-forbidden-files.sh
scripts/spike/test-isolated-run.sh  # sandbox self-test (Linux)
scripts/github-setup.sh --dry-run   # labels, milestones, default branch (needs gh auth + remote)
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cargo deny check
git cliff --unreleased              # preview the next changelog section
```

## Machines

macOS (Ghidra/REA + Rust), Fedora Linux game box (the client via `umu-run` inside the sandbox, Rust; REA works too), VPS (deploy). Claude's auto-memory does not travel between machines: anything that matters goes in this file or `docs/`. Per-OS setup: `docs/development.md`.
```

Delete the copied Caphras text above `## Pinned software`, and its old `## Machines` section. Then check: `grep -c '^## ' CLAUDE.md` prints 7.

- [ ] **Step 6: Write the docs**: `docs/README.md`, `docs/re/README.md`, `docs/re/sources.md`, `docs/protocol/README.md` (new content below), plus the edits to the copied `docs/development.md`.

`docs/README.md`:

```markdown
# Fenji docs

| Path | What it holds |
|---|---|
| [`superpowers/specs/2026-10-11-fenji-design.md`](superpowers/specs/2026-10-11-fenji-design.md) | The project design: ground rules, architecture, milestones. Start here. |
| `superpowers/plans/` | One implementation plan per milestone (M0–M5), written before that milestone starts. |
| [`development.md`](development.md) | Machine setup (macOS/Linux), branches, Conventional Commits, SemVer, changelog, releases. |
| [`re/README.md`](re/README.md) | Reverse-engineering workflow: pinned build, analysis setup, sandboxed client runs, what may be committed. |
| [`re/sources.md`](re/sources.md) | External sources we used, and sources rejected for clean-room reasons. |
| [`protocol/README.md`](protocol/README.md) | Our protocol findings (transport, opcode ledger, payloads) and their format. |

Ground rules (full text in the design spec, §2): legitimate copies only; clean room, no leaked server material; no game content in the repo; never touch live official services with modified clients; private and non-commercial.
```

`docs/re/README.md`:

````markdown
# Reverse-engineering workflow

How we analyse the pinned SoLO client without breaking the ground rules (design spec §2, §4).

## Where things live

| Thing | Location | Committed? |
|---|---|---|
| Pinned client copy (exe, DLLs, data archives) | `$FENJI_CLIENT_PATH` (default `~/fenji-client/`), outside the repo | Never |
| Ghidra project, REA sessions, evidence bundles, decompiled output, extracted scripts | `re/` in the repo root (git-ignored) | Never |
| Sandboxed runs: Wine prefix, logs, screenshots, stub logs | `re/spike/`, `re/stub-logs/` (git-ignored) | Never |
| Our own findings in prose | `docs/protocol/`, `docs/re/` | Yes |
| Rust definitions citing evidence | `crates/fenji-proto/`, `crates/fenji-data/`, `crates/fenji-lua/` | Yes |

`scripts/check-forbidden-files.sh` (run in CI and by the pre-commit hook) rejects game binaries, archives, Lua bytecode, captures and analysis databases.

## Getting the pinned build

The build we target is recorded in `docs/re/pinned-build.md`. Every copy comes from the person's own legitimately obtained client.

- **From your own install** (Steam or Gameforge's launcher). Copy it out of the library, so nothing else touches it and the sandbox can bind it; the sandbox refuses Steam directories:

  ```bash
  mkdir -p "$FENJI_CLIENT_PATH"
  cp -a --reflink=auto "<library>/steamapps/common/Swords of Legends Online/." "$FENJI_CLIENT_PATH/"
  ```

- **With DepotDownloader**, if SoLO is in your Steam library. You type your credentials into DepotDownloader; Fenji never sees them:

  ```bash
  DepotDownloader -app 1418100 -depot 1418101 -manifest 5002037851744157048 \
    -os windows -osarch 64 -username <your-steam-login> -dir "$FENJI_CLIENT_PATH"
  ```

Then compare `bin64/gujianol.exe`'s SHA-256 with `docs/re/pinned-build.md`. On the Mac, copy the same files for static analysis.

## Analysis setup

- **REA MCP server:** `rea-agents` (version pinned in `scripts/versions.env`), registered in `.mcp.json` through `scripts/rea-mcp.sh`.
- **Native engine:** **Ghidra 12.1.4** from the official release zip. Bootstrap installs it to `~/.local/share/fenji/tools/ghidra_12.1.4_PUBLIC`, and `~/.config/fenji/env.sh` exports `GHIDRA_INSTALL_DIR` and `JAVA_HOME` (JDK 21).
- **Role split:**
  - The Ghidra GUI project (`re/ghidra/fenji.gpr`, git-ignored) is the main analysis surface for `gujianol.exe`, and the canonical place for names, types and structs. The owner and Claude share it through the `ghidra` MCP server (the GhidraMCP extension plus the `bridge-mcp-ghidra` bridge to `127.0.0.1:8089`).
  - REA is the evidence ledger: Evidence IDs, unknowns, targeted decompilation and verification. Its full auto-analysis of the ~75,000-function executable is too slow to be the main surface.
  - Enable the plugin once in the GUI (File → Configure → Miscellaneous → GhidraMCP); `curl -s http://127.0.0.1:8089/check_connection` should then answer.
- **Readiness check** (changes nothing): `npx -y rea-agents@<version> doctor --client claude_code --json`.

## Running the client

Only inside `scripts/spike/isolated-run.sh` (design spec §2 rule 4). Inside it:
- the network is loopback only, and names come from a hosts file;
- there are no host daemon sockets;
- the display is a nested X server (`--x11 N`);
- the client runs non-root (`--no-root`, which umu needs).

Before a run:
- Quit Steam; the wrapper refuses to start while it runs.
- Bind only directories that belong to the run.
- Run the stub on loopback inside the same sandbox.

Read the wrapper's header before the first run.

## Working rules

- Static analysis of files on disk comes first. Runtime observation (Wine `WINEDEBUG`, the client's own log, stub sessions) happens only inside the sandbox, against our own stub or server.
- Every conclusion distinguishes observation, inference and unknown, and cites evidence: REA Evidence IDs, or pinned-exe addresses where REA can't reach. Unresolved questions go into REA with `record_unknown`.
- Decision gates (client patching, circumventing protection, the embedded kernel driver, Oodle licensing) stop the work until the owner decides.
- Close REA sessions (`close_binary`) when an investigation is finished. Never overwrite an evidence bundle; export to a new name.
- Community sources must be clearly licensed with clean provenance. List each in `docs/re/sources.md` with its licence and what we used it for. Rejected sources are listed there too, and anyone who has read one does no RE or protocol work.
````

`docs/re/sources.md`:

```markdown
# External sources

Every outside source we take hints from (design spec §2 rule 2, §4 C). Only clearly licensed material with clean provenance: nothing derived from leaked server files, binaries, scripts or source.

## Used

| Source (URL) | Licence | Provenance checked | What we used it for | Date |
|---|---|---|---|---|

No external sources used yet.

## Rejected

Material refused because it is leaked server material, or derives from it. Nobody doing Fenji RE or protocol work may read it.

| Source | Why rejected | Exposure | Date |
|---|---|---|---|
| "GuJian Online (古剑奇谭网络版) Server Reverse Engineering Report" (`GuJianOL-Server-RE-Report.md`, distributed in a RaGEZONE zip) | Its table of contents shows an analysis of the game's server package (server binaries, start script, server-side scripts), which the developer never published | One Claude session (the Caphras → Fenji transition) read its table of contents only and used nothing. That session coordinates Fenji but does no RE or protocol work | 2026-10-11 |
```

`docs/protocol/README.md`:

```markdown
# Protocol findings

Our own written findings about the pinned client's network protocol. No decompiled code or extracted scripts here: prose and tables only. Each fact cites its evidence: REA Evidence IDs, or pinned-exe addresses where REA can't reach.

## Files (created as milestones reach them)

| File | Contents | Milestone |
|---|---|---|
| `transport.md` | Framing, header scramble, Oodle Network codec, gate handshake and variant selection | M2 |
| `opcodes.md` | Opcode ledger (`STC_PROTOCOL_*`, `CTS_PROTOCOL_*`) | M2 onward |
| `payloads.md` | `pickle`/msgpack value encodings | M2 |
| `properties.md` | The synced-property (`prop_*`) schema, and how sync packets carry it | M2–M4 |
| `packets/<area>.md` | Per-opcode payload tables (`login`, `lobby`, `world`, `combat`, …) | M2–M5 |

## Opcode ledger format (`opcodes.md`)

| Value | Direction | Client name | Handler | Status | Evidence |
|---|---|---|---|---|---|
| `0` | C→S / S→C | `CTS_PROTOCOL_…` | native address, Lua script or `—` | `unknown` / `named` / `schema` / `verified` | Evidence |

Status meanings:

- `unknown`: value seen, purpose not known.
- `named`: the client's own name is known; the payload isn't mapped.
- `schema`: payload mapped from the client's code or scripts.
- `verified`: confirmed against the real client in a sandboxed stub session, and covered by a golden test.

## Payload table format (`packets/<area>.md`)

Payloads are `pickle`/msgpack-style tables rather than fixed layouts. For each opcode, give:
- the value and direction, and a one-line purpose;
- a field table (`key`, `type`, `meaning`, `notes`);
- the evidence, and the golden-test fixture name if one exists.
```

Edits to the copied `docs/development.md` (after the Step 1 rename):

- Machines table, Fedora row: replace `Rust work, running the pinned client via Proton against the stub/dev server, \`tcpdump\` captures, launcher testing; REA works here too` with `Rust work, running the pinned client via Proton inside the sandbox against the stub/dev server, launcher testing; REA works here too`.
- Delete the bullet starting `- Captures need capture rights:`.
- Replace the bullet `- Steam itself can be the RPM Fusion package or the Flatpak; Fenji doesn't depend on which. The pinned client runs through \`umu-run\`, not through Steam.` with `- Fenji doesn't need Steam. If it's installed (RPM Fusion or Flatpak), quit it before sandboxed runs. The pinned client runs through \`umu-run\`, never through Steam.`
- Replace the bullet `- The pinned client copy goes in \`$FENJI_CLIENT_PATH\` (default \`~/fenji-client\`). It is separate from the Steam-managed install.` with `- The pinned client copy goes in \`$FENJI_CLIENT_PATH\` (default \`~/fenji-client\`), outside any Steam or Gameforge library (the sandbox refuses binds inside Steam directories). See \`docs/re/README.md\`.`
- Scopes line: replace `proto`, `data`, `auth`, … with `proto`, `data`, `lua`, `auth`, `store`, `server`, `launcher`, `tools`, `content`, `deploy`, `docs`, `re`, `deps`, `release` (in that format).
- Replace `The pinned BDO client build is not part of our version.` with `The pinned SoLO client build is not part of our version.`

In `scripts/check-commit-subject.sh`, update the `Scopes:` help line to the same list, with `lua` after `data`.

- [ ] **Step 7: Templates, GitHub setup, CI.**

`.github/ISSUE_TEMPLATE/bug.yml`: the logs description becomes `Server/launcher logs. Never paste credentials, session tokens, game files or capture contents.`

`.github/ISSUE_TEMPLATE/decision-gate.yml`: the description becomes `Work has stopped at a gate (client patching, the embedded kernel driver, Oodle licensing, …) and needs the owner's decision.`

`.github/ISSUE_TEMPLATE/re-investigation.yml`:
- The `evidence` description becomes `What kind of evidence would answer it (static: functions/xrefs/strings/scripts; runtime: sandboxed stub session, Wine log, the client's own log).`
- The checkbox options become:

```yaml
        - label: Uses only my own legitimately obtained client and clean-provenance sources
          required: true
        - label: Nothing instrumented or modified connects to a live official service
          required: true
        - label: I have not read material listed as rejected in docs/re/sources.md
          required: true
```

`scripts/github-setup.sh`: replace the `LABELS=` and `MILESTONES=` blocks with:

```bash
# name|color|description
LABELS="type:task|1d76db|Planned unit of work (one task, one PR)
type:bug|d73a4a|Something behaves incorrectly
type:re|5319e7|Reverse-engineering investigation
decision-gate|b60205|Work stopped; needs the owner's decision
status:blocked|e4e669|Waiting on another issue or a decision
area:proto|0e8a16|fenji-proto: framing, scramble, codec, handshake, payloads
area:data|0e8a16|fenji-data: archives, tables, content overlays
area:lua|0e8a16|fenji-lua: bytecode, decompiler, VM
area:auth|0e8a16|fenji-auth: invites, accounts, tokens
area:store|0e8a16|fenji-store: persistence
area:server|0e8a16|fenji-server: listeners, world, admin
area:launcher|0e8a16|launcher crate and Tauri app
area:tools|0e8a16|archive, luatool, stub
area:deploy|0e8a16|VPS deployment
area:docs|0e8a16|documentation
area:dev-env|0e8a16|bootstrap, CI, sandbox, tooling"
```

```bash
# title|description
MILESTONES="M0 Dev environment + spike|Tooling ported, pinned build recorded, triage written up, sandbox runs the client non-root, one real-client run against the stub (release 0.1.0)
M1 Data layer|archive dumps items, skills, monsters, XP curve, maps; luatool reads the embedded Lua; Oodle-for-archives path decided (release 0.2.0)
M2 Transport + handshake|Stub completes the real client's gate handshake and decodes its first packets; Oodle Network path decided (release 0.3.0)
M3 Login + lobby|Invite accounts, launcher, ticket handoff, character list/create/delete in the real client (release 0.4.0)
M4 Social sandbox + VPS|Friends enter the starting zone, move, see each other, chat; admin commands; deployed (release 0.5.0)
M5 Grind loop (v1)|Monsters, AI, combat, XP, loot, inventory, persistence, live rates (release 1.0.0)"
```

`.github/workflows/ci.yml`:
- In `repo-hygiene`, delete the whole `Network isolation self-test` step (Task 3 brings the sandbox back).
- Change the ShellCheck run line to `shellcheck scripts/*.sh .githooks/*`.
- After the `Commit-subject check tests` step, add:

```yaml
      - name: Forbidden-file check tests
        run: scripts/test-check-forbidden-files.sh
```

- [ ] **Step 8: The stub crate.** After the Step 1 rename, check that `tools/stub/Cargo.toml` says `name = "fenji-stub"`, that the sources use `fenji_stub::`, and that `Cargo.lock` lists `fenji-stub`. `tools/stub/src/session.rs` writes `# fenji-stub session`. Then run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny check
```

Expected: all pass, with the stub's tests green.

- [ ] **Step 9: Verify there are no leftovers and that hygiene holds.**

```bash
grep -rnI -i -E 'caphras|black ?desert|pearl ?abyss|\bbdo\b|velia|\bpaz\b|xigncode' . \
  --exclude-dir=.git --exclude-dir=target --exclude-dir=re
```

Expected matches, and nothing else:
- the lineage lines in `docs/superpowers/specs/2026-10-11-fenji-design.md`, this plan, and `CLAUDE.md`'s first paragraph;
- the rejected-source row in `docs/re/sources.md` (only if it contains one of the words).

Then:

```bash
bash scripts/test-check-commit-subject.sh
bash scripts/test-check-forbidden-files.sh
scripts/check-forbidden-files.sh
shellcheck scripts/*.sh .githooks/*
uvx --from shellcheck-py==0.9.0.6 shellcheck scripts/*.sh .githooks/*
bash -n scripts/bootstrap.sh && scripts/bootstrap.sh --check || true
```

Expected: all pass. `bootstrap.sh --check` is allowed to report tools that aren't installed under the new `~/.local/share/fenji/tools` path yet; record its output in the report. Don't run the full bootstrap: it asks for sudo for `dnf`, and the owner runs it.

- [ ] **Step 10: Commit in logical steps** on `dev`. Activate the hooks first, so every commit is checked: `git config core.hooksPath .githooks`. Each commit message ends with the session's attribution trailers.

```bash
git add .gitignore .githooks .shellcheckrc scripts/check-forbidden-files.sh scripts/test-check-forbidden-files.sh \
        scripts/check-commit-subject.sh scripts/test-check-commit-subject.sh
git commit -m "chore: add repo guards for game content and commit messages"
git add docs/README.md docs/development.md docs/re docs/protocol
git commit -m "docs: add docs index, development guide, RE workflow and protocol notes"
git add scripts/versions.env scripts/bootstrap.sh scripts/rea-mcp.sh scripts/ghidra-mcp-bridge.constraints.txt rust-toolchain.toml
git commit -m "build: add pinned dev environment bootstrap for macOS and Fedora"
git add CLAUDE.md .claude/settings.json .mcp.json
git commit -m "chore: add CLAUDE.md and Claude Code project configuration"
git add .github/ISSUE_TEMPLATE .github/pull_request_template.md scripts/github-setup.sh cliff.toml
git commit -m "chore: add issue and PR templates and GitHub setup script"
git add Cargo.toml Cargo.lock deny.toml tools/stub
git commit -m "feat(tools): add fenji-stub protocol-discovery listener"
git add .github/workflows/ci.yml
git commit -m "ci: add CI workflow and shared commit-subject check"
git cliff -o CHANGELOG.md && git add CHANGELOG.md
git commit -m "docs: add changelog generated by git-cliff"
git status --short   # expected: empty
```

---

### Task 2: GitHub remote and issue tracking (the owner asked for the private repo on 2026-10-11)

**Files:** none in the repo.

**Interfaces:**
- Consumes: local `dev` from Task 1, and `scripts/github-setup.sh`.
- Produces: `origin` = `theintonerzero/fenji` (private), with `dev` as default and `main` pushed; labels, milestones M0–M5 and branch protection; one `Task` issue per Task 3–8, plus one tracking issue for the sandbox follow-ups.

- [ ] **Step 1: Create the repo and push.**

```bash
cd ~/projects/fenji
gh repo create theintonerzero/fenji --private --source . --remote origin \
  --description "Clean-room private server emulator for Swords of Legends Online (Rust)"
git branch main dev
git push -u origin dev main
```

- [ ] **Step 2: Labels, milestones, default branch:** `scripts/github-setup.sh --dry-run`, then `scripts/github-setup.sh`. Expected: 16 labels, 6 milestones, default branch `dev`.

- [ ] **Step 3: CI on `dev`:** `gh run watch "$(gh run list --branch dev --limit 1 --json databaseId -q '.[0].databaseId')"`. Expected: `rust`, `deny` and `repo-hygiene` all succeed.

- [ ] **Step 4: Branch protection on `dev`:** PR required, 0 approvals, required checks `rust` and `repo-hygiene` (strict off), admins not enforced:

```bash
gh api -X PUT repos/theintonerzero/fenji/branches/dev/protection --input - <<'JSON'
{"required_status_checks":{"strict":false,"contexts":["rust","repo-hygiene"]},
 "enforce_admins":false,
 "required_pull_request_reviews":{"required_approving_review_count":0},
 "restrictions":null}
JSON
```

- [ ] **Step 5: Issues.**
  - **Tasks 3–8:** one `Task` issue each (template fields: Goal, Plan reference `docs/superpowers/plans/2026-10-11-m0-dev-environment-and-spike.md, Task N`, Done when), in milestone `M0 Dev environment + spike`. Labels:
    - Tasks 3 and 4: `type:task` + `area:dev-env`;
    - Task 5: `type:task` + `area:docs`;
    - Tasks 6 and 7: `type:re` + `area:docs`;
    - Task 8: `type:task` + `area:docs`.
  - **Sandbox follow-ups:** one tracking issue, "Sandbox follow-ups carried over from Caphras's reviews" (`type:task`, `area:dev-env`, no milestone), with this checklist:
    - resolve the libXfont path via `/proc/$pid/root`;
    - pin the pipefail-carried "can't scan" refusal with a test;
    - generate `tr_TR.UTF-8` in CI so the locale test runs;
    - enforce no-DRI3 for the nested server;
    - refuse `-enable-ei-portal`;
    - close the check→bind window with an O_PATH fd and `--ro-bind-fd`;
    - the `pgrep -f` self-match;
    - the dead process-substitution fd that leaks inside;
    - decide the i386-ABI skip on the host;
    - the lo/`::1` check that flakes under heavy CPU load.

---

### Task 3: Port the sandbox *(area:dev-env)*

**Done when:** `scripts/spike/test-isolated-run.sh` ends with `all N checks passed` (skips allowed only where the environment lacks the feature) on the Fedora box and in CI.

**Files:**
- Create: `scripts/spike/isolated-run.sh`, `scripts/spike/test-isolated-run.sh`, `scripts/spike/seccomp-deny-keys.py`.
- Modify: `.github/workflows/ci.yml`.

**Interfaces:**
- Consumes: Caphras commit `1e23272`.
- Produces, for Tasks 4 and 7: `scripts/spike/isolated-run.sh [--hosts FILE] [--bind DIR]... [--ro-bind DIR]... [--x11 N] -- COMMAND [ARGS...]`, with the behaviour and exit codes its header documents.
  - The test-only environment variables are now `FENJI_ISOLATED_RUN_ALLOW_STEAM` and `FENJI_ISOLATED_RUN_TEST_EXTRA_FSTYPES`.

- [ ] **Step 1: Branch:** `git switch dev && git pull && git switch -c feat/dev-env-<issue>-sandbox`.

- [ ] **Step 2: Copy and rename:**

```bash
git -C ~/projects/caphras archive --format=tar 1e23272 -- scripts/spike | tar -x
sed -i -e 's/caphras/fenji/g' -e 's/Caphras/Fenji/g' -e 's/CAPHRAS/FENJI/g' scripts/spike/*
```

In `isolated-run.sh`'s header, replace `reach the internet or\n# Pearl Abyss's servers, look up their names` with `reach the internet or\n# the game's official services, look up their names`, keeping the line wrapping within 100 columns.

- [ ] **Step 3: Run the self-test:** `scripts/spike/test-isolated-run.sh`. Expected: `all N checks passed`. N is the same count Caphras reached at `1e23272`, give or take environment-dependent skips. If anything fails, compare with Caphras's run on the same machine before changing code.

- [ ] **Step 4: CI.** In `repo-hygiene`, change the ShellCheck line back to `shellcheck scripts/*.sh scripts/spike/*.sh .githooks/*`. Append the sandbox step, verbatim from Caphras's `ci.yml`:

```yaml
      - name: Network isolation self-test
        run: |
          sudo apt-get update
          # keyutils gives the self-test the keyctl CLI (the syscall-level key checks run without it).
          sudo apt-get install -y bubblewrap keyutils
          # Ubuntu 24.04 restricts unprivileged user namespaces via AppArmor by default; bwrap needs them.
          sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
          bwrap --version
          python3 -I scripts/spike/seccomp-deny-keys.py --disasm
          scripts/spike/test-isolated-run.sh
```

- [ ] **Step 5: Lint:** `shellcheck scripts/*.sh scripts/spike/*.sh .githooks/*` and the 0.9.0 variant via `uvx`. Then `scripts/check-forbidden-files.sh`.

- [ ] **Step 6: Commit, PR, CI green:** commit with `feat(re): add the loopback-only sandbox for client runs` (body: ported from Caphras `1e23272`, renamed; `Closes #<issue>`). Push, open the PR into `dev`, and watch `gh pr checks --watch`.

---

### Task 4: `--no-root` sandbox mode *(area:dev-env)*

**Why:** umu-launcher refuses to start as uid 0 (`[umu.__main__:82] ERROR: This script should never be run as the root user`), and the wrapper always runs COMMAND as uid 0 of its namespace. The owner chose an opt-in, strictly less privileged mode (2026-10-11).

**Done when:** `--no-root` runs COMMAND as the caller's uid and gid with no capabilities, everything else unchanged, and the self-test proves it locally and in CI.

**Files:** Modify `scripts/spike/isolated-run.sh` and `scripts/spike/test-isolated-run.sh`.

**Interfaces:**
- Produces, for Task 7: `--no-root` (idempotent). Exit 2 when the caller's uid or gid is 0 or 65534.

- [ ] **Step 1: Branch:** `git switch dev && git pull && git switch -c feat/dev-env-<issue>-no-root`.

- [ ] **Step 2: Write the failing tests first** in `test-isolated-run.sh` (same style: check/ok/skip/FAIL, traps before mktemp, no residue). Run each new check with `--no-root`:
  - `id -u` and `id -g` inside equal the caller's; `id -un` and `id -gn` print the caller's names (or `user`/`users` under the fallback in Step 3);
  - `CapEff`, `CapPrm` and `CapAmb` in `/proc/self/status` are all `0000000000000000`;
  - binding `127.0.0.1:80` fails, while a listener on a high loopback port is reachable from inside;
  - isolation is unchanged: only `lo`, no TCP to `1.1.1.1:443`, public names don't resolve (reuse the existing helpers);
  - the seccomp filter still applies: `add_key`/`keyctl` fail with EPERM (reuse the existing check);
  - a nested user namespace can be created inside: `unshare --user --map-root-user true` succeeds;
  - with the fake X server harness available, `--no-root --x11 N` works (`skip -` otherwise).

Run `scripts/spike/test-isolated-run.sh` and capture the failure: the new checks FAIL with `usage` / exit 2, because `--no-root` is unknown.

- [ ] **Step 3: Implement in `isolated-run.sh`.**
  - **Parse:** add `--no-root)` to the option `case`; it sets `noroot=1` and shifts one argument.
  - **Refuse** (exit 2 via `die2`) when `id -u` or `id -g` is `0` or `65534`. Those collide with the generated `root`/`nobody` entries, and running the wrapper as real root is out of scope.
  - **bwrap args:** with `noroot`, use `--uid "$(id -u)" --gid "$(id -g)"` and no `--cap-add` at all. Without it, keep `--uid 0 --gid 0 --cap-add CAP_NET_BIND_SERVICE --cap-add CAP_SETFCAP` exactly as now.
  - **Identity files:** with `noroot`, append `<name>:x:<uid>:<gid>::<home>:/bin/bash` to the generated passwd data and `<group>:x:<gid>:` to the group data.
    - The names come from `id -un`/`id -gn`.
    - If a name is empty, starts with `-`, or contains anything other than `[A-Za-z0-9._-]`, use `user`/`users` instead. Spell the character class out rather than using ranges, as the `--x11` digit check does.
  - **Header:**
    - add `[--no-root]` to the usage lines, and an option line;
    - rewrite the "Inside:" paragraph so it describes both modes (no capabilities with `--no-root`, so no ports below 1024; a nested bwrap such as pressure-vessel then maps the caller's uid itself);
    - add the exit-2 case.
    - `usage()` prints `sed -n '2,17p'`; adjust the range so it covers exactly the usage and option block after your edit.

- [ ] **Step 4: Run the self-test:** expected `all N checks passed` with the new checks included. Mutation-verify each new check on scratch copies outside the repo (disable it, show the covering test fails), and record the table in the PR body.

- [ ] **Step 5: Lint:** ShellCheck 0.11 and 0.9 (`uvx --from shellcheck-py==0.9.0.6 shellcheck scripts/*.sh scripts/spike/*.sh .githooks/*`), plus the forbidden-file check.

- [ ] **Step 6: Commit, PR, CI green:** `feat(re): add --no-root to isolated-run for tools that refuse uid 0` (`Closes #<issue>`).

---

### Task 5: Pin the client build *(area:docs; Fedora)*

**Done when:**
- `docs/re/pinned-build.md` records the AppID, depot → manifest, build ID, the main executable's path, size, SHA-256 and FileVersion, the file count and total size, how the copy was made, and the run-time artefacts found in it;
- the pinned copy exists in `$FENJI_CLIENT_PATH` (`~/fenji-client`) outside any Steam directory.

**Files:** Create `docs/re/pinned-build.md`; modify `docs/README.md` (add a row).

**Interfaces:** Produces the exe path and SHA-256 that Tasks 6–8 cite.

- [ ] **Step 1: Branch:** `git switch dev && git pull && git switch -c docs/re-<issue>-pinned-build`.

- [ ] **Step 2: Make the copy** (the source is the owner's Steam install, read-only; never modify it):

```bash
src="$HOME/.local/share/Steam/steamapps/common/Swords of Legends Online"
dst="$HOME/fenji-client"
[ ! -e "$dst" ] || [ -z "$(find "$dst" -mindepth 1 -print -quit)" ] || { echo "refusing: $dst is not empty"; exit 1; }
mkdir -p "$dst"
cp -a --reflink=always "$src/." "$dst/"
```

If `--reflink=always` fails (no CoW filesystem), stop and report; don't fall back to a 100 GB full copy without the controller's ruling. Confirm the copy is outside Steam: `realpath ~/fenji-client` must not start with `~/.local/share/Steam`, `~/.steam` or `~/.var/app/com.valvesoftware.Steam`.

- [ ] **Step 3: Record the identity:**

```bash
grep -E '"(appid|buildid|LastUpdated|SizeOnDisk)"' ~/.local/share/Steam/steamapps/appmanifest_1418100.acf
grep -A3 '"1418101"' ~/.local/share/Steam/steamapps/appmanifest_1418100.acf   # depot → manifest
stat -c %s ~/fenji-client/bin64/gujianol.exe
sha256sum ~/fenji-client/bin64/gujianol.exe
find ~/fenji-client -type f | wc -l
du -sb ~/fenji-client | cut -f1
```

Read FileVersion from the PE version resource with a stdlib-only Python reader, or from `strings -el bin64/gujianol.exe | grep -A1 FileVersion`. Expected: 2.1.19.42851.

- [ ] **Step 4: Find the owner's run-time artefacts:** list the files modified after the install finished (`LastUpdated` from the manifest, as epoch seconds):

```bash
last=$(grep -oP '"LastUpdated"\s+"\K[0-9]+' ~/.local/share/Steam/steamapps/appmanifest_1418100.acf)
find ~/fenji-client -type f -newermt "@$last" -printf '%P\t%s\n'
```

Delete the log files among them from the copy only: `bin64/gujianol.log`, and `Log_gujianol.htm` wherever it is. Keep any rewritten `settings/*.dat`. Record both lists in the doc.

- [ ] **Step 5: Write `docs/re/pinned-build.md`:**

```markdown
# Pinned client build

| Item | Value |
|---|---|
| Game | Swords of Legends Online (Gameforge's Western build of 古剑奇谭网络版) |
| Steam AppID | 1418100 (delisted 2023-06-01; servers shut down 2023-06-30) |
| Depot → manifest | 1418101 → 5002037851744157048 |
| Build ID | 10015727 |
| Main executable | `bin64/gujianol.exe`, <size> bytes, SHA-256 `<sha256>`, FileVersion 2.1.19.42851 |
| Files | <count> files, <bytes> bytes |
| Recorded | 2026-10-11, from the owner's Steam install on the Fedora box |

The pinned copy lives in `$FENJI_CLIENT_PATH` (default `~/fenji-client`). It is a reflink copy of the owner's Steam install, made outside the Steam library so that the sandbox can bind it and nothing else writes to it. See `docs/re/README.md` for how to make your own.

Run-time artefacts from the owner's one launch (2026-10-11), found in the install after it finished downloading:
- removed from the pinned copy: <list>
- kept (rewritten settings, harmless): <list>
```

Add to `docs/README.md`: ``| [`re/pinned-build.md`](re/pinned-build.md) | The exact SoLO build Fenji targets (AppID, depot manifest, exe SHA-256). |``

- [ ] **Step 6: Commit, PR, CI green:** `docs(re): record pinned client build` (`Closes #<issue>`).

---

### Task 6: Spike stage 1, the static triage write-up *(type:re, area:docs)*

Run by an agent that has never read rejected material (CLAUDE.md "Clean-room hygiene").

**Done when:** `docs/re/spike-0.md` §1–§7 state the static findings, each tagged **Observed** / **Observed (local)** / **Inferred** / **Unknown**, with evidence pointers, plus the list of questions left for Task 7.

**Files:**
- Create `docs/re/spike-0.md`; modify `docs/README.md` (add a row).
- Local only: copy `~/projects/caphras/re/solo/` to `~/projects/fenji/re/triage/`.

**Interfaces:**
- Consumes: the raw triage outputs and `triage-report.md`, and the exe SHA-256 from Task 5.
- Produces, for Task 7: the hostnames, URLs, ports, flags and config files to watch for.

- [ ] **Step 1: Branch and copy the evidence:** `git switch -c docs/re-<issue>-spike-0`, then `mkdir -p re && cp -a ~/projects/caphras/re/solo re/triage`.

- [ ] **Step 2: Re-verify the headline facts on the pinned copy** (not on the Steam install), with local stdlib tools only:
  - the section table and entropy;
  - the import count;
  - the `.pdata` entry count;
  - the presence of the `STC_PROTOCOL_`/`CTS_PROTOCOL_` strings;
  - the Lua header bytes;
  - the embedded driver resource;
  - the archive block magic.

  Keep the outputs in `re/triage/verify/`. A fact that doesn't reproduce is either dropped or marked Unknown.

- [ ] **Step 3: Write `docs/re/spike-0.md`** with these sections:
  - `# Spike 0: pinned client triage`
  - `## 1. Inventory, engine and middleware`
  - `## 2. Protection`
  - `## 3. Anti-cheat and the embedded AGT kernel driver`
  - `## 4. Network layer`
  - `## 5. Startup chain, server address and ticket`
  - `## 6. Data archives and scripts`
  - `## 7. The owner's launch (2026-10-11)`
  - `## Open for runtime (Task 7)`
  - `## Evidence index`

  Content rules:
  - Prose and tables only. No decompiled code, no script text, no copied disassembly listings; addresses, constants, enum names and file names are fine.
  - Redact personal data: the owner's Steam ID, account names, and the crash-reporter credentials the triage saw.
  - The evidence index maps each fact to a raw output file under `re/triage/` (local evidence), plus REA Evidence IDs wherever REA was used.
  - "Open for runtime" lists the names, ports, flags and config files to watch, and the AGT checks.

- [ ] **Step 4: Add the docs row:** ``| [`re/spike-0.md`](re/spike-0.md) | M0 spike: static triage of the pinned client, runtime observation, conclusions. |``. Run `scripts/check-forbidden-files.sh`.

- [ ] **Step 5: Commit, PR, CI green:** `docs(re): add spike-0 static triage of the pinned client` (`Closes #<issue>`).

---

### Task 7: Spike stage 2, an isolated runtime observation *(type:re; Fedora; needs the owner's nested display)*

Run by an agent that has never read rejected material.

**Done when:** `docs/re/spike-0.md` §8 reports what the pinned client does when started directly, without `SOLO_Launcher`, inside the sandbox with the stub listening:
- whether it reaches its login scene;
- which names it resolves and which ports it tries;
- whether the AGT driver is ever extracted or loaded;
- what, if anything, it sends to the stub.

**Files:** Modify `docs/re/spike-0.md`. Local only: `re/spike/` (prefix, Proton, umu runtime, hosts, run script, logs, screenshots) and `re/stub-logs/`.

**Interfaces:** Consumes `isolated-run.sh --no-root --x11 N` (Tasks 3–4), `fenji-stub` (Task 1), the pinned copy (Task 5), and the watch list (Task 6).

- [ ] **Step 1: Preconditions.**
  - `scripts/spike/test-isolated-run.sh` passes.
  - Steam is not running.
  - The owner has started the nested display, or has allowed `Bash(Xwayland :99:*)`: `Xwayland :99 -geometry 1280x720 -ac -nolisten tcp -noreset -extension MIT-SHM -shm`.
  - `/tmp/.X11-unix/X99` exists.

- [ ] **Step 2: Runtime files** (reflink copies from Caphras's local workspace; no network needed):

```bash
mkdir -p re/spike re/stub-logs
cp -a --reflink=auto ~/projects/caphras/re/spike/proton re/spike/proton
mkdir -p re/spike/home/.local/share
cp -a --reflink=auto ~/projects/caphras/re/spike/home/.local/share/umu re/spike/home/.local/share/umu
cp -a --reflink=auto ~/fenji-client re/spike/client     # throwaway copy: the run's writes never touch the pinned one
cargo build --release -p fenji-stub
```

- [ ] **Step 3: Create the Wine prefix inside the sandbox.** No game is involved; this proves umu and Proton work non-root:

```bash
S=$PWD/re/spike UMU_DIR=$(dirname "$(readlink -f ~/.local/bin/umu-run)")
scripts/spike/isolated-run.sh --no-root --x11 99 --bind "$S" --ro-bind "$UMU_DIR" -- \
  env HOME="$S/home" WINEPREFIX="$S/prefix" GAMEID=umu-default PROTONPATH="$S/proton/GE-Proton11-7" \
      UMU_RUNTIME_UPDATE=0 VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json \
  timeout -k 15 300 python3 "$UMU_DIR/umu-run" "" > re/spike/prefix-init.log 2>&1
```

Expected: `re/spike/prefix/drive_c` exists. A final `ShellExecuteEx failed: File not found` is expected, because no program was given; Caphras created its prefix the same way. If umu, pressure-vessel or Wine refuses for a new reason, **stop**: note the exact error in the issue and report. Don't improvise a weaker sandbox.

- [ ] **Step 4: Run script.** Write `re/spike/run.sh` (local). It runs the stub (`--listen` on each high port in `re/spike/ports`, `--out re/stub-logs/<run>`) and `gujianol.exe` from `client/bin64` under umu, inside one sandbox invocation: `--no-root --x11 99 --hosts re/spike/hosts --bind re/spike --bind re/stub-logs --ro-bind target/release --ro-bind $UMU_DIR`, with `WINEDEBUG=+winsock,+module,+process,+loaddll,warn+all` and a 300 s timeout. Per run, it saves:
  - Wine's log;
  - the client's own `gujianol.log`;
  - the exit code;
  - screenshots every 30 s: `import -display :99 -window root re/spike/out/<run>/screen-<t>.png`, from the host.

- [ ] **Step 5: Runs**, at most 4, varying one thing at a time:
  1. no arguments, an empty hosts file;
  2. the flags `SOLO_Launcher` passes (the values Task 6 recorded: `--steam-appid`, `--gf-region`, `--gf-appid` if known), still with no hosts entries;
  3. map every name runs 1–2 tried to resolve to `127.0.0.1`, with the stub on the high ports seen.

  Ports below 1024 can't be bound under `--no-root`. Record those attempts from Wine's log instead.

- [ ] **Step 6: Read the evidence.**
  - Names and ports: `grep -iE 'getaddrinfo|gethostbyname|connect' re/spike/out/*/wine.log`.
  - Every connect must target 127.0.0.1 or fail; anything else is a stop-and-report.
  - Modules: `grep -iE 'loaddll|\.sys|AGT|ProcessProtect|CreateService|NtLoadDriver' re/spike/out/*/wine.log`, plus a `find re/spike/prefix -newer re/spike/prefix-init.log -iname '*.sys'` check.
  - The client's log: the login-scene line and the server-list error.
  - Stub sessions: length and the first 16 bytes only.

- [ ] **Step 7: Write §8** of `docs/re/spike-0.md` as a run table:

```markdown
## 8. Runtime observation (isolated, stub only)

| Run | Change | Login scene? | Names resolved | Ports tried | AGT driver | Stub sessions |
|---|---|---|---|---|---|---|
```

Follow it with Observed/Inferred/Unknown bullets. If the client tries to load the driver, or anything would need patching to continue, open a `Decision gate` issue and stop.

- [ ] **Step 8: Commit, PR, CI green:** `docs(re): add spike-0 isolated runtime observation` (`Closes #<issue>`).

---

### Task 8: Spike conclusions, gates, M0 release *(area:docs; owner approval to release)*

**Done when:**
- `docs/re/spike-0.md` ends with conclusions for M0's questions and a recommendation for M1–M2;
- the spec's §12 table has an "Answer (M0)" column;
- every gate is resolved or open as an issue;
- with the owner's go-ahead, `v0.1.0` is tagged on `main`.

**Files:** Modify `docs/re/spike-0.md`, `docs/superpowers/specs/2026-10-11-fenji-design.md`, `Cargo.toml` (`version = "0.1.0"`), `CHANGELOG.md` (generated).

- [ ] **Step 1: Conclusions.** Append to `docs/re/spike-0.md`:

```markdown
## Conclusions

| Question | Answer | Confidence | Evidence |
|---|---|---|---|
| Does `gujianol.exe` reach its login scene without `SOLO_Launcher`, in the sandbox? | | | |
| Is the AGT kernel driver ever extracted or loaded? | | | |
| What does the client send first, and where does it get its server address? | | | |

## Recommendation for M1–M2

<2–5 sentences: proceed / change course; the redirect option from spec §5.3 the evidence favours; the first M1 and M2 targets>
```

- [ ] **Step 2: Spec §12:** add an "Answer (M0)" column linking to the `spike-0.md` sections. If an answer changes the design, add a short "M0 update" note in the affected spec section.

- [ ] **Step 3: Gates:** list every `decision-gate` issue in `spike-0.md` with its status. Don't release while a gate blocks M1.

- [ ] **Step 4: Commit and PR:** `docs(re): conclude spike-0 and update spec` → `dev`.

- [ ] **Step 5: Release 0.1.0 (only after the owner says go).**

```bash
git switch dev && git pull && git switch -c release/v0.1.0
sed -i 's/^version = "0.0.0"/version = "0.1.0"/' Cargo.toml
cargo check --workspace          # refreshes Cargo.lock
git cliff --tag v0.1.0 -o CHANGELOG.md
git commit -am "chore(release): v0.1.0"
git push -u origin HEAD
gh pr create --base main --title "chore(release): v0.1.0" --body "M0: dev environment + spike. See docs/re/spike-0.md."
# after CI passes and the owner merges with a merge commit:
git switch main && git pull && git tag -a v0.1.0 -m "v0.1.0" && git push origin v0.1.0
git switch dev && git merge --no-ff main -m "chore(release): merge v0.1.0 back into dev" && git push
gh release create v0.1.0 --title "v0.1.0 (M0)" --notes-from-tag
```

Expected: a `v0.1.0` tag on `main`, a `## [0.1.0]` section in `CHANGELOG.md`, and milestone M0 closed.
