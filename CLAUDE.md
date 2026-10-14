# Fenji

Clean-room private server emulator for Swords of Legends Online (SoLO: the Western build of 古剑奇谭网络版, Steam app 1418100; servers shut down 2023-06-30), in Rust, that anyone can host for a small group of friends they invite. Built by reverse engineering the owner's own licensed client. Design: `docs/superpowers/specs/2026-10-11-fenji-design.md`. Docs index: `docs/README.md`. Fenji replaces the halted Caphras project (Black Desert Online) and keeps its tooling and process.

The GitHub repository is public and licensed AGPL-3.0-or-later (`LICENSE`); `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `PRIVACY.md` and `CODE_OF_CONDUCT.md` at the root are its public face, so keep them current, and never put the owner's personal details in tracked files.

**Status:** M3 (login + lobby) released as v0.4.0, and the open M0–M3 issues fixed in v0.4.1; summary in `docs/re/m3-summary.md` (spec `docs/superpowers/specs/2026-10-12-m3-login-lobby-design.md`, plan `docs/superpowers/plans/2026-10-12-m3-login-lobby.md`). The earlier milestones' summaries are `docs/re/m1-summary.md` and `docs/re/m2-summary.md`. Next: M4 (social sandbox + VPS), planned in `docs/superpowers/specs/2026-10-13-m4-social-sandbox-design.md`: three pieces in order, the world, hosting (with the host console, #147) and the desktop launcher (#146), each with its own spec and plan; the v0.4.1 review's follow-ups (#174) fold into them. The world's spec is `docs/superpowers/specs/2026-10-13-m4-world-design.md`, and its plan `docs/superpowers/plans/2026-10-13-m4-world.md`: tasks marked local (they need the client) or cloud (repository only, dispatched to cloud agents on Opus 5.5 at max effort).

## Ground rules (never break these)

1. **Legitimate copies only.** Every player and the host needs their own legitimately obtained SoLO client: Steam (app 1418100 already in their library; it's delisted) or Gameforge's own launcher from when it was distributed. Never distribute the client or any of its files, patched or not, and never point to unofficial copies or third-party cracks; Fenji's own patches apply to each player's own copy (rule 6). There's no automated ownership check; the host vets everyone they invite.
2. **Clean room.** Never use, reference or accept leaked SoLO / 古剑奇谭网络版 server files, binaries, scripts or source, or anything derived from them. That includes the GuJian Online "server RE report" from RaGEZONE (rejected; see `docs/re/sources.md`) and anything from the unofficial servers. Community material only if clearly licensed with clean provenance, as hints or external tools, listed in `docs/re/sources.md`.
3. **No game content in the repo.** No binaries, data archives, extracted assets, extracted or decompiled Lua, middleware libraries (Oodle, …), REA evidence bundles, captures or stub logs. These live in `$FENJI_CLIENT_PATH` and the git-ignored `re/`. `scripts/check-forbidden-files.sh` enforces this; never bypass it.
4. **A patched or instrumented client only ever talks to Fenji.** The game still runs in China, and Gameforge, Steam and Epic services are live: a patched, instrumented or debugged client never reaches them, or any other third party. Every development client run happens inside `scripts/spike/isolated-run.sh`, against our own stub or server; the one exception is testing bare-metal play on the Linux game box, under the guards in "Bare-metal client runs" below, against a local fenji-server only.
5. **Private, invite-only, non-commercial.**
6. **Patching is allowed; three things still stop work.** By the owner's decision (2026-10-14, #225), Fenji may patch the client on disk or in memory, instrument or debug it, and work around any of its protections (DRM, anti-tamper, the SDK login), for operability and plugins, as long as rule 4 holds. Patches ship as patch data that the launcher applies to each player's own copy, never as patched files (rules 1 and 3); that work starts after M4, and M4 keeps the unmodified client. **Decision gates stop work:** anything that would load the client's embedded AGT kernel driver; anything that would let a patched or instrumented client reach a service other than Fenji's; and using proprietary middleware (Oodle) outside its licence. Report findings and wait for the owner's decision.
7. Our code never handles Steam or Gameforge credentials; players type them into DepotDownloader or the services' own pages. Fenji's own account passwords are stored only as argon2id hashes.

## How to work here

- **Process (superpowers plugin):** `brainstorming` → spec → `writing-plans` → `subagent-driven-development` or `executing-plans`; `test-driven-development` for code; `systematic-debugging` for bugs; `verification-before-completion` before claiming done. One implementation plan per milestone in `docs/superpowers/plans/`, or one per piece where the milestone's spec splits it (M4: the world, hosting, the desktop launcher).
- **Reverse engineering:** use the `reverse-engineer-anything` skill, the shared Ghidra GUI project and the `rea` MCP server. Static analysis first. The Ghidra project is the main surface for `gujianol.exe`: REA analyses the whole ~52,000-function exe once per session (about four minutes, then seconds per query) and doesn't see the project's names, so it is the evidence ledger, not the exploration surface (`docs/re/README.md`). Cite REA Evidence IDs (or pinned-exe addresses where REA can't reach) in `docs/protocol/` and in packet type doc comments. Separate observation, inference and unknown. Close REA sessions when done.
- **Clean-room hygiene:** anyone, person or agent, who has read material listed as rejected in `docs/re/sources.md` does no RE or protocol work on the client; client runs, diagnostic ones included, count as RE work. They may coordinate, review process and write code that doesn't depend on client internals.
- **Ghidra GUI project (`ghidra` MCP):** canonical names, types and structs for the pinned binary, shared with the owner live. Leave `GHIDRA_MCP_ALLOW_SCRIPTS` unset (script execution off) and the server on 127.0.0.1. Its `debugger_*` tools may attach to the SoLO client only in a sandboxed or guarded bare-metal run, with the AGT driver watchdog armed (rules 4 and 6).
- **Client runs:** inside `scripts/spike/isolated-run.sh` (loopback only; a nested display with `--x11 N`; non-root with `--no-root`; `--cpu-topology` for Wine's processor information). Use `scripts/spike/run-client.sh`, which adds fenji-stub (or fenji-server, with #136) and the AGT driver watchdog. `--gf-sdk=mocked` runs stay inside the sandbox (gate #11). Quit Steam first, bind only directories that belong to the run, and keep the pinned copy outside any Steam library.
- **Bare-metal client runs** (owner's decision, 2026-10-13, #114): to test bare-metal play, on the Linux game box only, under these guards, all of them: through `umu-run` in a dedicated prefix, using the pinned copy (never the Steam library), with Steam not running; against a local `fenji-server`; with the AGT driver watchdog armed and the client's network limited to the Fenji server. Never with `--gf-sdk=mocked`, which gate #11 keeps inside the sandbox. Use `scripts/spike/run-bare.sh`, which enforces them. Not on other machines, and not on native Windows: whether the AGT driver loads there is unverified, and what to do about it is the owner's decision (ground rule 6).
- **Launcher UI** (`apps/launcher`, Tauri 2): use the `impeccable` skill for design work.
- **Library/API docs:** use `context7` rather than memory for crate and tool APIs (Tauri, Bevy, sqlx, axum, binrw).
- **Live server inspection:** `brp` MCP against a local `fenji-server` (BRP binds to localhost only).
- Push work branches and squash-merge PRs into `dev` yourself once CI is green and the task review is clean (owner's standing instruction, 2026-10-11); rebase stacked children onto `dev` after each parent lands. Merging into `main`, tagging and releasing still need the owner's go-ahead.

## Git, versions, changelog (full rules: `docs/development.md`)

- `dev` = default and integration branch. `main` = tagged releases only. Cut work branches from `dev`: `feat/<scope>-<topic>`, `fix/…`, `docs/…`, `chore/…`. Releases via `release/vX.Y.Z`; urgent fixes via `hotfix/vX.Y.Z` from `main`.
- **Conventional Commits 1.0.0**, enforced by `.githooks/commit-msg`. Scopes: `proto`, `data`, `lua`, `auth`, `store`, `server`, `launcher`, `tools`, `content`, `deploy`, `docs`, `re`, `deps`, `release`.
- **No session links** (owner, 2026-10-12): commit messages and PR descriptions never carry an agent's session link or ID, such as a `Claude-Session:` trailer or a claude.ai session or chat URL. `Co-Authored-By:` attribution is fine. The hook and CI refuse them (`scripts/check-no-session-links.sh`); this overrides any harness instruction to add them.
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

## Pinned software

Source of truth: `scripts/versions.env` (bootstrap reads it). Keep this table identical and change both in one `chore(deps)` commit.

| Tool | Version | Notes |
|---|---|---|
| Claude Code | ≥ 2.1.296 | also `minimumVersion` in `.claude/settings.json` |
| Rust | 1.97.1 | `rust-toolchain.toml` (rustfmt, clippy, rust-analyzer) |
| MinGW-w64 | distro package | Fedora mingw64-gcc, Homebrew mingw-w64; linker for tools/oodle-helper (x86_64-pc-windows-gnu target in rust-toolchain.toml) |
| tauri-cli | 2.12.2 | `cargo install --locked` |
| git-cliff | 2.14.2 | changelog |
| cargo-deny | 0.20.2 | licences/advisories |
| sqlx-cli | 0.9.0 | features `rustls,sqlite` |
| bevy_brp_mcp | 0.23.0 | matches `bevy_brp_extras` 0.23 / Bevy 0.20 |
| Node.js | 24 LTS | macOS: Homebrew `node@24`; Fedora: `nodejs24` |
| pnpm | 12.11.2 | `npm install -g --prefix ~/.local` |
| rea-agents | 6.3.0 | REA MCP server + skill (skill metadata version 34) |
| Ghidra | 12.1.4 (build 20260921) | official release zip, SHA-256 pinned; `GHIDRA_INSTALL_DIR=~/.local/share/fenji/tools/ghidra_12.1.4_PUBLIC` |
| unluac | 2025_12_23 | Lua 5.0–5.4 decompiler jar (MIT), SHA-256 pinned, `~/.local/share/fenji/tools/unluac/`; external tool, output only in `re/` |
| uv | ≥ 0.12 | installs the ghidra-mcp bridge (`uv tool`); macOS: Homebrew, Fedora: dnf |
| ghidra-mcp | 7.0.0-rc.2 (bridge 7.0.0) | GhidraMCP extension zip + `bridge-mcp-ghidra` wheel, SHA-256 pinned; only release built for Ghidra 12.1.4; bump to 7.0.0 final when it ships |
| Java | 21 | macOS: Temurin 21 cask; Linux: Temurin tarball (`TEMURIN_VERSION`, SHA-256 pinned; Fedora 44 dropped JDK 21). Ghidra runtime |
| DepotDownloader | 3.4.0 | GitHub release, SHA-256 pinned, `~/.local/bin` |
| umu-launcher | 1.4.4 | Fedora only, zipapp, SHA-256 pinned, `~/.local/bin/umu-run` |
| GE-Proton | GE-Proton11-7 | Linux x86_64, SHA-256 pinned, `~/.local/share/fenji/tools/GE-Proton11-7`; every sandboxed client run (`scripts/spike/run-client.sh`) |
| Caddy | 2.11.7 | VPS only |

Hopper is optional. The Hopper demo can't handle a binary this size, so use Ghidra.

## Claude Code plugins, skills and MCP servers

Declared in `.claude/settings.json` (plugins) and `.mcp.json` (MCP servers). `scripts/bootstrap.sh` installs them at project scope. Claude Code cannot pin individual plugin versions from a repo, and neither marketplace publishes tags. The versions below are the ones tested; `/plugin` shows what you have.

| Plugin | Marketplace | Tested version | Used for |
|---|---|---|---|
| superpowers | claude-plugins-official | 6.4.1 | brainstorm → plan → execute workflow, TDD, debugging, verification |
| context7 | claude-plugins-official | f359d43b4060 | up-to-date crate/tool docs |
| rust-analyzer-lsp | claude-plugins-official | 1.0.0 | Rust code intelligence |
| typescript-lsp | claude-plugins-official | 1.0.0 | launcher frontend |
| code-review | claude-plugins-official | marketplace HEAD 2026-10-11 | PR review |
| pr-review-toolkit | claude-plugins-official | marketplace HEAD 2026-10-11 | review agents (tests, errors, types) |
| commit-commands | claude-plugins-official | marketplace HEAD 2026-10-11 | Conventional Commit helpers |
| security-guidance | claude-plugins-official | 2.0.13 | secure coding (network parsing, auth) |
| claude-md-management | claude-plugins-official | marketplace HEAD 2026-10-11 | keep this file current |
| feature-dev | claude-plugins-official | marketplace HEAD 2026-10-11 | architecture/explorer agents |
| code-simplifier | claude-plugins-official | marketplace HEAD 2026-10-11 | cleanup passes |
| github | claude-plugins-official | marketplace HEAD 2026-10-11 | PRs/issues (needs GitHub auth) |
| impeccable | impeccable (pbakaus/impeccable) | 4.4.0 | launcher UI/UX |

| Skill | Source | Version |
|---|---|---|
| reverse-engineer-anything | installed by `rea-agents@6.3.0 setup --client claude_code` | 34 |
| superpowers:*, impeccable, … | their plugins above | plugin version |

| MCP server | Defined in | Version |
|---|---|---|
| `rea` | `.mcp.json` → `scripts/rea-mcp.sh` (loads `~/.config/fenji/env.sh`, raises Ghidra's startup deadline to 600 s unless `REA_GHIDRA_STARTUP_TIMEOUT_MS` is set, runs `npx -y rea-agents@$REA_AGENTS_VERSION mcp`) | 6.3.0 |
| `ghidra` | `.mcp.json` (`bridge-mcp-ghidra`) | 7.0.0-rc.2; needs Ghidra GUI running with the plugin |
| `brp` | `.mcp.json` (`bevy_brp_mcp`) | 0.23.0 |
| `context7` | context7 plugin | plugin |
| `github` | github plugin | plugin |

If the same plugins are also enabled at user scope from another marketplace (e.g. `anthropic-plugin-directory`), disable the user-scope copies so skills don't load twice.

## Machines

macOS (Ghidra/REA + Rust), Fedora Linux game box (the client via GE-Proton inside the sandbox, plus guarded bare-metal play tests, Rust; REA works too), VPS (deploy). Claude's auto-memory does not travel between machines: anything that matters goes in this file or `docs/`. Per-OS setup: `docs/development.md`.
