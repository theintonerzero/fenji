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

- `dnf` installs build tools, Tauri prerequisites (`webkit2gtk4.1-devel`, `libxdo-devel`, `libappindicator-gtk3-devel`, `librsvg2-devel`, `openssl-devel`), `alsa-lib-devel` (Bevy audio, pulled in by `bevy_remote`; needed by `bevy_brp_mcp`) and `nodejs24`. Fedora Atomic (Silverblue/Kinoite/Bazzite) gets the same list to layer or use in a toolbox.
- Ghidra comes from the official release zip on both macOS and Fedora (SHA-256 verified), so both machines run the identical pinned build at `~/.local/share/fenji/tools/ghidra_12.1.4_PUBLIC`.
- macOS gets Java from the Temurin 21 cask and Node from Homebrew `node@24`. On Linux, bootstrap downloads the Temurin 21 tarball (SHA-256 pinned) into `~/.local/share/fenji/tools`, because Fedora 44 no longer packages JDK 21.
- `umu-run` (Proton outside Steam), `DepotDownloader`, `ghidraRun`, `bridge-mcp-ghidra` (via `uv tool`) and pnpm land in `~/.local/bin`; the GhidraMCP extension lands in Ghidra's user directory (`~/.config/ghidra/ghidra_12.1.4_PUBLIC/Extensions` on Linux, `~/Library/ghidra/...` on macOS); the unluac jar lands in `~/.local/share/fenji/tools/unluac/` (used by `luatool decompile`); GE-Proton (Linux x86_64) lands in `~/.local/share/fenji/tools/GE-Proton11-7`.
- Fenji doesn't need Steam. If it's installed (RPM Fusion or Flatpak), quit it before sandboxed runs. The pinned client never runs through Steam, and every client run on this machine goes through the sandbox, except the guarded bare-metal play tests CLAUDE.md describes. Sandboxed runs start GE-Proton's `proton` script directly, because umu's runtime container fails inside the sandbox (seen without `--cpu-topology`; untested with it). `umu-run` is what `fenji-launch play` starts the client with on Linux, and what the guarded bare-metal runner uses (`scripts/spike/run-bare.sh`).
- `tools/oodle-helper` is a Windows program (M2): `rust-toolchain.toml` adds the `x86_64-pc-windows-gnu` target, and bootstrap installs its MinGW-w64 linker (Fedora `mingw64-gcc`, Homebrew `mingw-w64`).
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

`<type>(<scope>)<!>: <description>`. The `commit-msg` hook enforces this; CI checks PR titles the same way. A subject is at most 100 characters. A PR title becomes its squash commit's subject with GitHub's ` (#N)` after it, so CI holds it to 100 minus that suffix: 93 characters for a three-digit PR.

- Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
- Scopes: `proto`, `data`, `lua`, `auth`, `store`, `server`, `launcher`, `tools`, `content`, `deploy`, `docs`, `re`, `deps`, `release`.
- `!` or a `BREAKING CHANGE:` footer marks a breaking change.
- No session links: a commit message or PR description never carries an AI agent's session link or ID (a `Claude-Session:` trailer, a claude.ai session or chat URL). The hook refuses them in commit messages, and CI in a PR's title, description and commits that aren't in `dev` yet (`scripts/check-no-session-links.sh`, `scripts/check-pr-session-links.sh`, in the required `repo-hygiene` job). `Co-Authored-By:` attribution is fine.

## Versioning: SemVer 2.0.0

- One version for the whole monorepo: `[workspace.package] version` in `Cargo.toml`. Every crate inherits it (`version.workspace = true`). The desktop launcher's `apps/launcher/package.json` and `apps/launcher/src-tauri/Cargo.toml` carry it too (its `tauri.conf.json` has none of its own), kept equal by the release steps below and checked in CI by `scripts/check-launcher-version.sh`.
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
# bump the version in Cargo.toml's [workspace.package], apps/launcher/package.json and
# apps/launcher/src-tauri/Cargo.toml (scripts/check-launcher-version.sh says whether they agree)
cargo check --workspace          # refreshes Cargo.lock; CI builds with --locked
(cd apps/launcher/src-tauri && cargo check)   # and the launcher shell's own Cargo.lock
# set CLAUDE.md's **Status:** line: this milestone released as vX.Y.Z, and which milestone is next;
# and the public files' status: README.md's status paragraph and milestone table, and the version
# PRIVACY.md and SECURITY.md name; and docs/hosting.md's VERSION= examples and the versions in its
# example folder names (fenji-server-X.Y.Z-…): X.Y.Z where it installs, and the next patch where it
# updates
# from v0.5.4 on, the release key's line must be in deploy/allowed_signers, the update script and
# docs/hosting.md (scripts/set-release-key.sh KEY.pub writes all three), or the sums job fails
git cliff --tag vX.Y.Z -o CHANGELOG.md
# stage the release files only, so no unrelated local edit rides into the release commit
git add Cargo.toml Cargo.lock CHANGELOG.md CLAUDE.md README.md PRIVACY.md SECURITY.md docs/hosting.md \
  apps/launcher/package.json apps/launcher/src-tauri/Cargo.toml apps/launcher/src-tauri/Cargo.lock
git commit -m "chore(release): vX.Y.Z"
# PR release/vX.Y.Z → main, merge commit, then:
git switch main && git pull && git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z
# wait for the release workflow, which attaches the server's archives, the launcher's bundles with
# their updater signatures, latest.json and SHA256SUMS to the tag's draft release; its sign job
# waits for the owner to approve the release environment's deployment (the run's page, "Review
# deployments"), and gh run watch waits with it
until run=$(gh run list --workflow release.yml --branch vX.Y.Z --json databaseId --jq '.[0].databaseId // empty') &&
  [ -n "$run" ]; do sleep 5; done
gh run watch "$run" --exit-status
# check the draft: an archive for each of the server's four targets; the launcher's AppImage,
# Windows installer (-setup.exe) and Fenji.app.tar.gz, each with its .sig; latest.json, which names
# all three; and SHA256SUMS, which matches them all
draft=$(mktemp -d) && gh release download vX.Y.Z --dir "$draft" && ls -l "$draft"
(cd "$draft" && sha256sum -c SHA256SUMS)   # macOS: shasum -a 256 -c SHA256SUMS
# merge main back into dev
git switch dev && git merge --no-ff main && git push
# release notes: a line naming the pinned build (from docs/re/pinned-build.md), then the changelog section
notes=$(mktemp)
{ echo "Pinned SoLO client: build <build ID>, bin64/gujianol.exe SHA-256 <exe SHA-256> (docs/re/pinned-build.md)."
  echo
  echo "The launcher isn't signed by Microsoft or Apple (launcher spec L6). To open it the first time:"
  echo "on Windows, choose More info, then Run anyway; on macOS, open Fenji once, then choose Open Anyway"
  echo "in System Settings, Privacy & Security."
  echo
  git cliff --latest --strip all; } > "$notes"
# publish the draft, with its notes
gh release edit vX.Y.Z --draft=false --notes-file "$notes"
# close the milestone's release issue (PRs into main don't auto-close it) and the milestone
gh issue close <release issue>
m=$(gh api 'repos/{owner}/{repo}/milestones' --jq '.[] | select(.title == "<milestone title>") | .number')
gh api -X PATCH "repos/{owner}/{repo}/milestones/$m" -f state=closed
```

The release workflow (`.github/workflows/release.yml`) builds `fenji-server` for four targets, each on a runner of its own system: Linux on x86_64 and arm64 (static, with musl), Windows on x86_64, and macOS on Apple silicon. `scripts/package-server.sh` packs each build with its system's start script (`deploy/start/`), `fenji.example.toml`, `README.txt` (`deploy/archive/`), `LICENSE`, `SOURCE.txt` and `THIRD-PARTY-NOTICES.txt`, and `scripts/test-package-server.sh` checks what it packs. The notices are the licences of every crate the target's build links or runs in its proc-macros, which cargo-about (pinned in `scripts/versions.env`) collects from the locked dependency graph, and, for the static Linux builds, musl's notice (`deploy/archive/musl-COPYRIGHT`). cargo-about's `deploy/archive/about.toml` accepts exactly the licences `deny.toml` allows, and the test fails while the two lists differ. For a crate whose own licence text cargo-about can't find or read, it puts in the licence's standard text, which for MIT and BSD leaves the copyright line a template (`<year> <copyright holders>`), so the script refuses notices that hold one, and names the crates: each needs a checksum-pinned clarification in `about.toml` (#328). CI's `notices` job runs the script on every pull request for each of the workflow's server targets, around a stand-in for the binary, so a dependency change that needs one fails there rather than on a tag. Each build job checks its archive's entries with its own system's tools, and the publish job attaches nothing unless the tag still points at the run's commit. A pull request that changes the workflow, the packaging scripts, what they pack (`deploy/start/`, `deploy/archive/`, the launcher's `tauri.conf.json`) or `scripts/versions.env` runs it as a dry run: it builds every archive and bundle, unsigned, and publishes nothing, and the files stay with the run. So does a run by hand from the Actions tab.

The same workflow builds the desktop launcher's bundles, each on its own runner: the AppImage on Linux (x86_64), the NSIS installer on Windows (x86_64) and the app on macOS (Apple silicon), with `cargo tauri build --bundles <bundle>`. Before each build, `scripts/launcher-notices.sh` writes the bundle's third-party notices (#341). They hold the licences of the shell's crates for the bundle's target, which cargo-about collects from the shell's own lock with `deploy/launcher/about.toml`; it accepts exactly the shell's `deny.toml` licences, has the shell's clarifications, and says where the source of the crates under the MPL-2.0 is. Then come the licence files of the view's production npm packages and the fonts' OFL texts. The script refuses notices with a placeholder where a copyright line belongs (`scripts/check-notices.sh`, as for the server), and `scripts/test-launcher-notices.sh` tests it. Each bundle carries the notices at the root of its resources, as `THIRD-PARTY-NOTICES.txt` (`bundle.resources`, which the build passes with `--config`), and the Settings popover's Licences opens them. CI's `launcher` job makes the Linux notices on every pull request. On a tag, the tag must match the launcher's version. The builds never see a key. After them, on a tag (the hosting spec's Amendment D, HD2):
- **`sign`**, in the `release` environment (below), signs each bundle for the updater with `cargo tauri signer sign` (launcher spec §3.8, L5), from a fresh tauri-cli with no cache. It signs a bundle only when it's the one the launcher job reported, and reports each signature in turn.
- **`sums`**, in the `release` environment too, builds and installs nothing, so no third-party code runs beside the release's SSH key. It takes each file only when it's the one its own job reported (`scripts/check-release-files.sh`). Any job of a run can replace another job's artifact, or add one, but none can change another job's outputs: so each job that makes a file reports its SHA-256 among its outputs, and an artifact that holds anything but what its job reported fails the release. It writes the updater's `latest.json` with `scripts/launcher-latest-json.sh`, whose notes are the tag's section of `CHANGELOG.md` (`scripts/release-notes.sh`): the changelog is committed before the tag, so no git-cliff runs in a release. It checks that the update script's key line is one of `deploy/allowed_signers`'s. Then it writes `SHA256SUMS` over every file the release attaches, signs it as `SHA256SUMS.sig` (`scripts/sign-sums.sh`), and checks the signature against `deploy/allowed_signers`. A secret that isn't the key's private half, or a repository without the key's line, fails the release there. `scripts/test-sign-sums.sh` and `scripts/test-release-files.sh` test the scripts.
- **`publish`** checks what `sums` put together as a host would, the signature and then every sum, and attaches it only when `SHA256SUMS` lists every file.

On a pull request, or a run by hand, **`check`** takes the files as `sums` would, without signing anything, so a change that breaks a job's report fails before a tag.

So the owner approves the environment twice per release: `sign`, then `sums`. The installed launchers read `latest.json` from the latest release, so the draft must be published, not just made, before players get the update. `scripts/test-launcher-latest-json.sh` checks the script.

**The release key.** Its private half lives only on the owner's game box, in `~/.config/fenji/release-key/` (readable by the owner only), and in the secrets `TAURI_SIGNING_PRIVATE_KEY` (the key file's text) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` of the repository's `release` environment, whose deployment rule admits `v*` tags only: GitHub itself keeps them from any other run, whatever its workflow file says, and only the release workflow's `sign` job names that environment. It never goes into any repository. The environment also has the owner as its required reviewer, with no bypass for administrators: each release's `sign` job waits until the owner approves it, so a pushed tag alone can't sign an update. Its public half is in `apps/launcher/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`, key ID 02D8621117AE26ED). A launcher only installs updates signed with that key, so losing the private half means a new key, a release that carries the new public half, and players updating by hand once.

**The release's SSH key** (the hosting spec's Amendment D). It signs each release's `SHA256SUMS`, which the update script checks. Its private half lives only:
- on the owner's game box, beside the launcher's key, in `~/.config/fenji/release-key/` (readable by the owner only), under a passphrase;
- in the `release` environment's secret `SSH_SIGNING_PRIVATE_KEY`, without one, behind the same deployment rule and approval.

The owner makes it once. The first line asks for a passphrase. The secret's copy goes through a temporary file without it, which the last step removes:

```sh
ssh-keygen -t ed25519 -C 'Fenji releases' -f ~/.config/fenji/release-key/ssh-release
plain=$(mktemp) && cp ~/.config/fenji/release-key/ssh-release "$plain" && ssh-keygen -p -N '' -f "$plain" &&
  gh secret set SSH_SIGNING_PRIVATE_KEY --env release < "$plain"; rm -f "$plain"
scripts/set-release-key.sh ~/.config/fenji/release-key/ssh-release.pub
```

The last line writes the key's public half, as an allowed-signers line, into three places: `deploy/allowed_signers`, the update script, and sections 4 and 5 of `docs/hosting.md`. A test keeps them agreeing.

**A new key** (HD7), when the old one may go out of use:
1. Make the new key as above, but keep the secret as it is.
2. Run `scripts/set-release-key.sh --next NEW.pub`. The update script and the guide then trust the new key, while `deploy/allowed_signers` keeps the old key's line beside it.
3. Release. That release is signed with the old key, so the scripts hosts run now accept it, and it brings them the new line.
4. Then set the secret to the new key, and run `scripts/set-release-key.sh NEW.pub` (without `--next`), which leaves the new key's line alone.

If the old key is lost or leaked, skip steps 2 and 3. Hosts' scripts then refuse the next release, saying to update by hand once (section 5 of the guide), which brings the new line.

## Checks before a PR

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
scripts/check-forbidden-files.sh
scripts/check-unsafe-allowed.sh
```

The workspace forbids `unsafe_code`. The server and the Oodle helper deny it instead, so that one file each may allow it: `crates/fenji-server/src/unix.rs` and `tools/oodle-helper/src/dll.rs`. `scripts/check-unsafe-allowed.sh` refuses an allow anywhere else (CI's `repo-hygiene` job).

A change under `apps/launcher/` also runs the launcher's checks (CI's `launcher` job), below.

## The desktop launcher

`apps/launcher` is the desktop launcher (spec: `docs/superpowers/specs/2026-10-15-m4-launcher-design.md`). Its model, with all its state and secrets, is the `fenji-launcher` crate in the workspace; `apps/launcher/src-tauri` is a thin Tauri 2 shell, a Cargo workspace of its own outside the repository's; `apps/launcher/src` is the TypeScript view. The model's tests write the view's types (`src/gen/`) and check the golden snapshots (`fixtures/`); see `apps/launcher/fixtures/README.md`.

Building the shell needs Node 24, pnpm 12.11.2 and tauri-cli 2.12.2, and on Linux WebKitGTK's development packages; `scripts/bootstrap.sh` installs them all on Fedora and macOS (CI's `launcher` job lists Ubuntu's packages).

```bash
cd apps/launcher
pnpm install --frozen-lockfile
pnpm run check && pnpm test && pnpm run build && node scripts/check-licences.mjs
cd src-tauri
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check
cargo tauri dev                  # the launcher with the view served live by Vite
cargo tauri build --no-bundle    # a release build, without the installers
```

The launcher keeps its settings in `~/.config/fenji-launcher/` and its data (the log, fetched tools, the Wine prefix) in `~/.local/share/fenji-launcher/` on Linux. To try it without touching yours, start it with `XDG_CONFIG_HOME` and `XDG_DATA_HOME` pointing at a scratch folder.
