# Contributing to Fenji

Thanks for your interest. Fenji is a clean-room server emulator for Swords of Legends Online, written in Rust and built by studying our own licensed copies of the game client. The rules below protect the project, so please read them before you open an issue or a pull request.

## The ground rules

These come from the design spec ([§2](docs/superpowers/specs/2026-10-11-fenji-design.md#2-ground-rules-non-negotiable)). A contribution that breaks one can't be accepted, however good it is.

1. **Your own legitimate copy of the game.** If your work needs the client, use your own copy: Steam (app 1418100, which you can download only if it's already in your library, since it was delisted) or Gameforge's launcher from when it was distributed. Never share a copy or any of its files, patched or not, link to one, ask for one, or work with third-party cracks.
2. **Clean room.** Never use, reference or submit leaked server files, server binaries, server scripts or server source for Swords of Legends Online / 古剑奇谭网络版, or anything derived from them. That includes write-ups based on leaks and anything from the unofficial servers. [`docs/re/sources.md`](docs/re/sources.md) lists the sources we use and the ones we rejected.
3. **No game content in the repository.** No game binaries or data archives, no extracted assets, no extracted or decompiled scripts, no middleware libraries (Oodle or others), no packet captures, no stub logs, no analysis databases or evidence bundles. They stay on your machine, in the git-ignored `re/` folder. `scripts/check-forbidden-files.sh` checks every PR.
4. **Never touch official services.** A patched or instrumented client, and every Fenji tool, only ever talks to a Fenji stub or server, never to a live service or any other third party: the game still runs in China, and Gameforge's, Steam's and Epic's services are live. A developer's client runs happen inside the sandbox (`scripts/spike/run-client.sh`), against our own stub or server. The one exception is the guarded bare-metal play tests on a Linux machine, through `scripts/spike/run-bare.sh` (CLAUDE.md, "Bare-metal client runs"). Players start the client with `fenji-launch play`, against their own host's server.
5. **Private and non-commercial.** Fenji is for private, invite-only, unlisted, non-commercial servers. Contributions that add public server listings, payments or monetisation won't be accepted.
6. **Some steps need the maintainer's decision first:** anything that could load the client's kernel driver, anything that could let a patched or instrumented client reach a service other than Fenji's, and using proprietary middleware outside its licence. If your work reaches one, stop and open a **Decision gate** issue. Patching the client and working around its protections are allowed within rule 4 (#225): patches ship as patch data applied to each player's own copy, never as patched files. Decompressing the game's Oodle archives with an open-source decompressor, such as ooz, is allowed too; Epic's own Oodle library, outside the sandbox, isn't.
7. **No third-party credentials.** Fenji never handles Steam or Gameforge passwords.

## Who may do reverse engineering

If you have ever read material rule 2 rules out (for example the rejected sources in `docs/re/sources.md`), you can still contribute. Work that doesn't depend on the client's internals is welcome: tooling, CI, the launcher's interface, and docs and reviews that don't touch the client's internals. But please don't do reverse engineering or protocol work on the client, and that includes running it. Tell us in your PR if this applies to you; nobody will think less of you for it.

By opening a pull request you confirm that it contains nothing that rule 2 or rule 3 rules out.

## Getting set up

Building and testing needs only Rust (through rustup; `rust-toolchain.toml` pins the version) and a C compiler. [`docs/development.md`](docs/development.md) covers the rest: `scripts/bootstrap.sh` sets up the whole reverse-engineering environment on macOS or Linux (it installs system packages and asks before using `sudo` or editing your shell's start-up file), and the doc explains the branches and the release process. Reverse-engineering work also needs [`docs/re/README.md`](docs/re/README.md): the pinned client build, the analysis setup, the sandbox, and how findings cite their evidence.

## How work flows

- **Start from an issue.** Bugs, tasks, questions, reverse-engineering questions and decision gates each have an issue template. For anything bigger than a small fix, open or comment on an issue first so we can agree the approach.
- **Branch from `dev`**, named `feat/<scope>-<issue>-<topic>`, `fix/…`, `docs/…` or `chore/…`, for example `feat/proto-12-handshake-state-machine`.
- **One concern per pull request**, into `dev`, with `Closes #N`. Don't mix refactoring with behaviour changes.
- **Conventional Commits.** Titles look like `feat(proto): add the hello check`. The `commit-msg` hook in `.githooks/` (enable it with `git config core.hooksPath .githooks`; bootstrap does this) and CI enforce it; the scopes are listed in `docs/development.md`. The changelog is generated from these, so don't edit `CHANGELOG.md` by hand. If an AI tool helps you, leave its session links out of commit messages and PR descriptions (CI refuses them); a `Co-Authored-By:` line is welcome.
- **Tests first** for code: write the failing test, then the fix. Code that parses bytes from the network or from files must never panic on bad input; it returns an error.
- **Before you push:**

  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo deny check
  scripts/check-forbidden-files.sh
  ```

- **Review and merge.** A maintainer reviews, and once CI is green the PR is squash-merged into `dev`. Releases go to `main`, tagged, by the maintainer.

## Reverse-engineering findings

Findings go in `docs/protocol/` or `docs/re/` as prose and tables, never as decompiled code or disassembly listings. Addresses, constants and function names are fine. Tag each finding **Observed**, **Inferred** or **Unknown**, and cite its evidence: an Evidence ID from REA (the reverse-engineering tool the project uses through MCP) with the exact call, or a pinned-exe address ([`docs/re/README.md`](docs/re/README.md), "Evidence for `gujianol.exe`"). New dependencies must use a licence `deny.toml` allows.

## AI coding assistants

The repository is set up for Claude Code (`CLAUDE.md`, `.claude/`, `.mcp.json`). If you use an assistant, the same rules apply to it, and you're responsible for what it submits.

## Licence of contributions

Fenji is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE). By contributing, you agree that your contribution is licensed under the same terms, and you confirm that you have the right to submit it.

## Conduct and security

Everyone taking part follows the [code of conduct](CODE_OF_CONDUCT.md). Report security problems privately, as [SECURITY.md](SECURITY.md) explains, never in a public issue.
