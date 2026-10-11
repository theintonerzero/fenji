# Development workflow

## Machines

| Machine | Use it for |
|---|---|
| macOS (arm64) | Rust work, REA static analysis (Ghidra), docs |
| Fedora Linux game box | Rust work, running the pinned client via Proton inside the sandbox against the stub/dev server, launcher testing; REA works here too |
| VPS | Deployment only (see `deploy/`, design spec §8) |

Everything needed to continue the project lives in the repo: `CLAUDE.md`, `docs/`, `scripts/`. Claude Code's auto-memory is per machine and does not travel. If something matters across machines, it goes in `CLAUDE.md` or `docs/`.

## Setting up a machine (macOS or Linux)

```bash
git clone <remote> fenji && cd fenji
git switch dev
scripts/bootstrap.sh          # installs/updates everything pinned in scripts/versions.env
exec $SHELL -l                # pick up ~/.config/fenji/env.sh
scripts/bootstrap.sh --check  # every row should say "ok"
claude                        # accept workspace trust; approve the rea and brp MCP servers
```

Fedora notes (the Linux box runs Fedora; other distros get a printed package list to install themselves):

- `dnf` installs build tools, Tauri prerequisites (`webkit2gtk4.1-devel`, `libxdo-devel`, `libappindicator-gtk3-devel`, `librsvg2-devel`, `openssl-devel`), `alsa-lib-devel` (Bevy audio, pulled in by `bevy_remote`; needed by `bevy_brp_mcp`), `tcpdump`, `wireshark-cli` (tshark) and `nodejs24`. Fedora Atomic (Silverblue/Kinoite/Bazzite) gets the same list to layer or use in a toolbox.
- Ghidra comes from the official release zip on both macOS and Fedora (SHA-256 verified), so both machines run the identical pinned build at `~/.local/share/fenji/tools/ghidra_12.1.4_PUBLIC`.
- macOS gets Java from the Temurin 21 cask and Node from Homebrew `node@24`. On Linux, bootstrap downloads the Temurin 21 tarball (SHA-256 pinned) into `~/.local/share/fenji/tools`, because Fedora 44 no longer packages JDK 21.
- `umu-run` (Proton outside Steam), `DepotDownloader`, `ghidraRun`, `bridge-mcp-ghidra` (via `uv tool`) and pnpm land in `~/.local/bin`; the GhidraMCP extension lands in Ghidra's user directory (`~/.config/ghidra/ghidra_12.1.4_PUBLIC/Extensions` on Linux, `~/Library/ghidra/...` on macOS).
- Fenji doesn't need Steam. If it's installed (RPM Fusion or Flatpak), quit it before sandboxed runs. The pinned client never runs through Steam. Sandboxed runs start GE-Proton's `proton` script directly, because umu's runtime container fails inside the sandbox; `umu-run` is for plain play outside the sandbox.
- The pinned client copy goes in `$FENJI_CLIENT_PATH` (default `~/fenji-client`), outside any Steam or Gameforge library (the sandbox refuses binds inside Steam directories). See `docs/re/README.md`.

## Branches

| Branch | Role |
|---|---|
| `dev` | Default and integration branch. All feature work merges here. |
| `main` | Releases only. Every commit on `main` is a tagged release. |
| `feat/<scope>-<topic>`, `fix/…`, `docs/…`, `chore/…`, `refactor/…`, `test/…`, `ci/…` | Work branches, always cut from `dev`, merged back into `dev` by PR. |
| `release/vX.Y.Z` | Cut from `dev` to prepare a release; merged into `main` (tagged), then back into `dev`. |
| `hotfix/vX.Y.Z` | Cut from `main` for urgent fixes; merged into `main` (tagged), then back into `dev`. |

PRs into `dev` are squash-merged with a Conventional Commit title. Release and hotfix branches are merged with a merge commit so tags stay reachable from both branches.

## Issues and PRs

- **Issues track all work.** Each milestone is a GitHub milestone (M0–M5). When a milestone's implementation plan is approved, every plan task becomes a `Task` issue in that milestone. Bugs, RE questions and decision gates use their own issue templates (`.github/ISSUE_TEMPLATE/`).
- **One PR per concern.** A PR does one thing: one plan task, one bug fix, one refactor, one docs change. Don't mix refactoring or formatting with behaviour changes, or RE findings docs with unrelated code. If the description needs "and also…", split it.
- Every PR links its issue (`Closes #N`) and targets `dev`, except `release/*` and `hotfix/*`, which target `main`.
- Branch names include the issue number when there is one: `feat/proto-12-handshake-state-machine`.
- Labels: `type:*` (task, bug, re), `area:*` (crate or subsystem), `decision-gate`, `status:blocked`.
- `scripts/github-setup.sh` creates the labels and milestones and makes `dev` the default branch (idempotent; `--dry-run` first).

## Commits: Conventional Commits 1.0.0

`<type>(<scope>)<!>: <description>`. The `commit-msg` hook enforces this; CI checks PR titles the same way.

- Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
- Scopes: `proto`, `data`, `lua`, `auth`, `store`, `server`, `launcher`, `tools`, `content`, `deploy`, `docs`, `re`, `deps`, `release`.
- `!` or a `BREAKING CHANGE:` footer marks a breaking change.

## Versioning: SemVer 2.0.0

- One version for the whole monorepo: `[workspace.package] version` in `Cargo.toml`. Every crate inherits it (`version.workspace = true`). The launcher's `tauri.conf.json` and `package.json` are kept equal by the release steps below.
- Tags: `vX.Y.Z` on `main`. Release candidates: `vX.Y.Z-rc.N` on the release branch.
- Before 1.0.0, each milestone release bumps the **minor** version: M0 → `0.1.0`, M1 → `0.2.0`, M2 → `0.3.0`, M3 → `0.4.0`, M4 → `0.5.0`. M5 (grind loop, v1) → `1.0.0`.
- Between milestones: fixes bump **patch**.
- From 1.0.0: breaking change to config, saved data or launcher↔server compatibility → **major**; new feature → **minor**; fix → **patch**.
- The pinned SoLO client build is not part of our version. Each release's notes state which pinned build it supports. Changing the pinned build is at least a minor bump (major after 1.0.0).

## Changelog

`CHANGELOG.md` follows Keep a Changelog 1.1.0 and is **generated** from commits by `git-cliff` (`cliff.toml`). Don't edit it by hand; write good commit messages instead. `git cliff --unreleased` previews what the next release will say.

## Releasing

```bash
git switch dev && git pull
git switch -c release/vX.Y.Z
# bump [workspace.package] version in Cargo.toml (+ launcher tauri.conf.json / package.json once they exist)
git cliff --tag vX.Y.Z -o CHANGELOG.md
git commit -am "chore(release): vX.Y.Z"
# PR release/vX.Y.Z → main, merge commit, then:
git switch main && git pull && git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z
# merge main back into dev
git switch dev && git merge --no-ff main && git push
```

## Checks before a PR

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
scripts/check-forbidden-files.sh
```
