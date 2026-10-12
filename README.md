# Fenji

[![CI](https://github.com/theintonerzero/fenji/actions/workflows/ci.yml/badge.svg?branch=dev)](https://github.com/theintonerzero/fenji/actions/workflows/ci.yml)
[![Licence: AGPL-3.0-or-later](https://img.shields.io/badge/licence-AGPL--3.0--or--later-blue.svg)](LICENSE)

Fenji is a clean-room private server emulator for **Swords of Legends Online** (SoLO), written in Rust. SoLO is the Western release of 古剑奇谭网络版 (Gujian Online). Gameforge ran it from 2021 until its servers shut down on 30 June 2023, and the game was delisted from Steam a month earlier. Fenji's goal is modest: anyone can host a Fenji server and invite their friends, each with their own copy of the game, to play together again.

All the protocol and format knowledge here comes from studying our own licensed copies of the client and from watching it talk to our own test server, inside a sandbox. No leaked server code or files are used, and no game files are in this repository.

## Status

**Not playable yet.** The real client connects to Fenji's test server, completes the transport handshake and sends its first message, but it stays on the login screen: nothing after that exists yet.

| Milestone | What it delivers | State |
|---|---|---|
| M0 | Development environment, sandbox, first analysis of the client | Released, v0.1.0 |
| M1 | Reading the client's data: archives, embedded scripts | Released, v0.2.0 |
| M2 | The network transport and handshake, against the real client | Released, v0.3.0 |
| M3 | Accounts, the launcher, character list and creation | Next |
| M4 | Friends in the starting zone: moving, seeing each other, chat | Planned |
| M5 | Monsters, combat, XP, loot and saved progress (v1) | Planned |

The details are in the [design spec](docs/superpowers/specs/2026-10-11-fenji-design.md) and the [docs index](docs/README.md). Each milestone ends with a summary, such as [M2's](docs/re/m2-summary.md).

## The ground rules

- **Bring your own legitimate copy.** Every player, and whoever hosts the server, needs their own legitimate copy of the game: on Steam (app 1418100, which you can download only if it's already in your library, since it was delisted) or through Gameforge's launcher back when Gameforge distributed it. Fenji supports one client build, the one in [`docs/re/pinned-build.md`](docs/re/pinned-build.md) (Steam build 10015727); other copies may not work. Fenji never distributes the game, never points to unofficial copies and doesn't support cracked or modified clients.
- **Clean room.** No leaked server files, binaries, scripts or source, and nothing derived from them, ever.
- **No game content here.** No game files, extracted assets, decompiled scripts, middleware libraries or packet captures. The tools read the game's data from your own installation, at run time.
- **Hands off official services.** Fenji never connects a modified or instrumented client to a live service. Testing happens in a sandbox, or on the project's own Linux machine with the client's network limited to a local Fenji server, and always against our own server.
- **Private and free.** Fenji is meant for private, invite-only, unlisted and non-commercial servers. The project doesn't run, list or vouch for any server, and won't help with public or paid ones.

[CONTRIBUTING.md](CONTRIBUTING.md) has the full rules and what they mean for contributors.

## What's in the repository

| Path | What it is |
|---|---|
| `crates/fenji-proto` | The client's network transport without I/O: the handshake, encryption, frames and message encoding. |
| `crates/fenji-data` | Reads the client's own data: its archives, file index, embedded file system and table cache. |
| `crates/fenji-lua` | The client's Lua 5.3 dialect: parser, disassembler, normaliser. |
| `tools/` | Command-line tools: `stub` (test server), `archive`, `luatool`, and `oodle-helper`. |
| `scripts/` | Machine setup, the sandbox for client runs, and repository checks. |
| `docs/` | The design, what we've learned about the client, and how we work. |

The server, the account service and the launcher come in later milestones.

## For developers

Building and testing needs Rust, installed through [rustup](https://rustup.rs) (the repository pins its toolchain in `rust-toolchain.toml`), and a C compiler, on macOS or Linux:

```bash
git clone https://github.com/theintonerzero/fenji.git && cd fenji
git switch dev
cargo test --workspace
```

`scripts/bootstrap.sh` sets up the whole environment the project uses for reverse engineering and client runs: Ghidra, REA, GE-Proton, the sandbox's tools and more. It installs system packages, and asks before it uses `sudo` or edits your shell's start-up file; `--check` compares what's installed with the pins. Client runs need Linux (Fedora is what we use). [`docs/development.md`](docs/development.md) explains the setup, branches, commits and releases. Reverse-engineering work starts at [`docs/re/README.md`](docs/re/README.md).

## Contributing, security and privacy

- [CONTRIBUTING.md](CONTRIBUTING.md): how to help, and the clean-room rules.
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md): how we treat each other.
- [SECURITY.md](SECURITY.md): report vulnerabilities privately, never in a public issue.
- [PRIVACY.md](PRIVACY.md): what data Fenji handles.

## Legal

Fenji is an independent project. It is not affiliated with, endorsed by or sponsored by Aurogon, Wangyuan Shengtang, Gameforge, Valve or Epic Games. Swords of Legends Online, 古剑奇谭 and every other game name, trademark and asset belong to their owners.

The game's terms likely forbid reverse engineering it or running it against unofficial servers. The game is still operated in China, and its publishers do pursue private servers, so hosting or joining one is at your own risk. The law differs from place to place, and you are responsible for how you use this software.

Copyright © 2026 The Fenji contributors. Fenji is free software: you can redistribute it and/or modify it under the terms of the [GNU Affero General Public License](LICENSE), version 3 or (at your option) any later version. It comes with no warranty. If you host a modified version for other people, the AGPL requires you to offer those players its source code.
