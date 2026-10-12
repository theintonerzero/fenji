# Fenji

[![CI](https://github.com/theintonerzero/fenji/actions/workflows/ci.yml/badge.svg?branch=dev)](https://github.com/theintonerzero/fenji/actions/workflows/ci.yml)
[![Licence: AGPL-3.0-or-later](https://img.shields.io/badge/licence-AGPL--3.0--or--later-blue.svg)](LICENSE)

Fenji is a clean-room private server emulator for **Swords of Legends Online** (SoLO), written in Rust. SoLO is the Western release of 古剑奇谭网络版 (Gujian Online). Gameforge ran it from 2021 until its servers shut down on 30 June 2023, and the game was delisted from Steam a month earlier. Fenji's goal is modest: anyone can host a Fenji server and invite their friends, each with their own copy of the game, to play together again.

Everything here comes from studying our own licensed copies of the client and from watching it talk to our own test server, inside a sandbox. No leaked server code or files are used, and no game files are in this repository.

## Status

**Not playable yet.** The real client connects to Fenji's test server and completes its network login, but the title screen is as far as it gets: nothing after that exists yet.

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

- **Bring your own legitimate copy.** Every player, and whoever hosts the server, needs their own copy of the game: on Steam (app 1418100, downloadable only if it's already in your library) or from Gameforge's launcher while it was distributed there. Fenji never distributes the game, never points to unofficial copies and doesn't support cracked or modified clients.
- **Clean room.** No leaked server files, binaries, scripts or source, and nothing derived from them, ever.
- **No game content here.** No game files, extracted assets, decompiled scripts, middleware libraries or packet captures. The tools read the game's data from your own installation, at run time.
- **Hands off official services.** Fenji never connects a modified or instrumented client to a live service. All testing happens in a sandbox against our own server.
- **Private and free.** Fenji servers are invite-only and never monetised.

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

You need macOS or Linux; client runs need Linux (Fedora is what we use).

```bash
git clone https://github.com/theintonerzero/fenji.git && cd fenji
git switch dev
scripts/bootstrap.sh          # installs the pinned toolchain and tools
scripts/bootstrap.sh --check  # every row should say "ok"
cargo test --workspace
```

[`docs/development.md`](docs/development.md) explains the setup, branches, commits and releases. Reverse-engineering work starts at [`docs/re/README.md`](docs/re/README.md).

## Contributing, security and privacy

- [CONTRIBUTING.md](CONTRIBUTING.md): how to help, and the clean-room rules.
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md): how we treat each other.
- [SECURITY.md](SECURITY.md): report vulnerabilities privately, never in a public issue.
- [PRIVACY.md](PRIVACY.md): what data Fenji handles.

## Legal

Fenji is an independent project. It is not affiliated with, endorsed by or sponsored by Aurogon, Wangyuan Shengtang, Gameforge, Valve or Epic Games. Swords of Legends Online, 古剑奇谭 and every other game name, trademark and asset belong to their owners.

The game's own terms may forbid reverse engineering it or running it against unofficial servers, and the law differs from place to place. You are responsible for how you use this software.

Copyright © 2026 The Fenji contributors. Fenji is free software: you can redistribute it and/or modify it under the terms of the [GNU Affero General Public License](LICENSE), version 3 or (at your option) any later version. It comes with no warranty.
