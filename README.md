# Fenji

[![CI](https://github.com/theintonerzero/fenji/actions/workflows/ci.yml/badge.svg?branch=dev)](https://github.com/theintonerzero/fenji/actions/workflows/ci.yml)
[![Licence: AGPL-3.0-or-later](https://img.shields.io/badge/licence-AGPL--3.0--or--later-blue.svg)](LICENSE)

Fenji is a clean-room private server emulator for **Swords of Legends Online** (SoLO), written in Rust. SoLO is the Western release of 古剑奇谭网络版 (Gujian Online). Gameforge ran it from 2021 until its servers shut down on 30 June 2023, and the game was delisted from Steam a month earlier. Fenji's goal is modest: anyone can host a Fenji server and invite their friends, each with their own copy of the game, to play together again.

All the protocol and format knowledge here comes from studying our own licensed copies of the client and from watching it talk to our own test server, inside a sandbox. No leaked server code or files are used, and no game files are in this repository.

## Status

**Not a game yet.** A Fenji server runs invite-only accounts, the character lobby and a shared world (M3, M4). The real, unmodified client signs in with a single-use ticket, lists, creates and deletes characters, and enters the starting zone. There, players see each other, move and see each other move, and chat nearby, to the whole server or in whispers, and the server's admins have `!tp`, `!kick`, `!announce` and `!invite`. Each character comes back where it last stood. There are no monsters, combat or quests until M5. The desktop launcher makes your account from an invite link, checks your copy of the game or downloads it from Steam, and starts it on Linux; on Windows and macOS it can't start the game yet. Anyone can host a server for the friends they invite, on their own computer or on a VPS, with a console as a Minecraft server has ([the guide for hosts](docs/hosting.md)). The game connection between the client and the server is readable and unauthenticated. It carries no password, but play over a trusted network or a VPN ([PRIVACY.md](PRIVACY.md)). Joining a server means trusting its host: the game client obeys server messages that can write files and run code on your computer. Fenji's server never sends them, but a changed server could ([PRIVACY.md](PRIVACY.md#trusting-a-servers-host)).

| Milestone | What it delivers | State |
|---|---|---|
| M0 | Development environment, sandbox, first analysis of the client | Released, v0.1.0 |
| M1 | Reading the client's data: archives, embedded scripts | Released, v0.2.0 |
| M2 | The network transport and handshake, against the real client | Released, v0.3.0 |
| M3 | Accounts, the launcher, character list and creation | Released, v0.4.0 |
| M4 | Friends in the starting zone: moving, seeing each other, chat | Released, v0.5.0 |
| M5 | Monsters, combat, XP, loot and saved progress (v1) | Planned |

The details are in the [design spec](docs/superpowers/specs/2026-10-11-fenji-design.md) and the [docs index](docs/README.md). Each milestone ends with a summary, such as [M4's](docs/re/m4-summary.md).

## The ground rules

- **Bring your own legitimate copy.** Every player, and whoever hosts the server, needs their own legitimate copy of the game: on Steam (app 1418100, which you can download only if it's already in your library, since it was delisted) or through Gameforge's launcher back when Gameforge distributed it. Fenji supports one client build, the one in [`docs/re/pinned-build.md`](docs/re/pinned-build.md) (Steam build 10015727); other copies may not work. Fenji never distributes the game or any of its files, patched or not, and never points to unofficial copies or third-party cracks.
- **Clean room.** No leaked server files, binaries, scripts or source, and nothing derived from them, ever.
- **No game content here.** No game files, extracted assets, decompiled scripts, middleware libraries or packet captures. The tools read the game's data from your own installation, at run time.
- **Patches for Fenji only.** Fenji may patch the client so it works fully with Fenji and can take plugins, and the launcher will apply those patches to each player's own copy (#225; after M4). A patched or instrumented client only ever talks to a Fenji server, never to an official or any other service.
- **Hands off official services.** Testing happens in a sandbox, or on the project's own Linux machine with the client's network limited to a local Fenji server, and always against our own server.
- **Private and free.** Fenji is meant for private, invite-only, unlisted and non-commercial servers. The project doesn't run, list or vouch for any server, and won't help with public or paid ones.

[CONTRIBUTING.md](CONTRIBUTING.md) has the full rules and what they mean for contributors.

## What's in the repository

| Path | What it is |
|---|---|
| `crates/fenji-proto` | The client's network transport without I/O: the handshake, encryption, frames and message encoding, and the lobby's, the world's and chat's messages. |
| `crates/fenji-server` | The server: the account API (invites, accounts, sign-in, game tokens), the gate that takes the client's login, the character lobby, and the world: who sees whom, movement, chat and admin commands. |
| `crates/fenji-console` | The host console: live logs above and a command line below, full-screen in a terminal or as plain lines, and what a server and its consoles say to each other. |
| `crates/fenji-store` | The server's SQLite store: accounts, invites, sessions, game tokens and characters. |
| `crates/fenji-auth` | Password hashing (argon2id) and the invite and token formats. |
| `crates/fenji-launcher` | The launcher's library, which holds all its state and secrets, and `fenji-launch`, its command line: redeem an invite, check the client folder against the pinned build, and play (on Linux, through umu-launcher). |
| `apps/launcher` | The desktop launcher for players: a Tauri 2 window over `fenji-launcher`'s model. It pastes an invite link, makes your account, checks or downloads your game, plays, and updates itself. |
| `crates/fenji-data` | Reads the client's own data: its archives, file index, embedded file system and table cache. |
| `crates/fenji-oodle` | An open decoder for the Oodle Kraken compression of the client's archives: a safe Rust port of [ooz](https://github.com/powzix/ooz), GPL-3.0-or-later. |
| `crates/fenji-lua` | The client's Lua 5.3 dialect: parser, disassembler, normaliser. |
| `tools/` | Command-line tools: `stub` (test server), `archive`, `luatool`, and `oodle-helper`. |
| `deploy/` | What releases and hosts use: the server's start scripts and archive files, the reference deployment's systemd units and Caddyfile, and the launcher's notices. |
| `scripts/` | Machine setup, the sandbox for client runs, and repository checks. |
| `docs/` | The design, what we've learned about the client, how we work, and [the guide for hosts](docs/hosting.md). |

Monsters, combat and the rest of the game come in M5.

## Playing

You need the desktop launcher, an invite link from a friend who hosts a Fenji server, and your own copy of the game (above).

1. **Get the launcher** from the [latest release](https://github.com/theintonerzero/fenji/releases/latest), from v0.5.0: the AppImage on Linux, the installer on Windows, or the app on macOS. Microsoft and Apple haven't signed it. So the first time, on Windows choose More info, then Run anyway. On macOS, open it once, then choose Open Anyway in System Settings, Privacy & Security.
2. **Paste the invite link** your host sent you, and make your account: a name and a password for that server.
3. **Show it your game.** Use the copy Steam installed, which the launcher finds by itself, or choose your copy's folder; the launcher checks it against the build Fenji supports. On Linux it can also download your copy from Steam instead: you sign in by scanning the code the launcher draws with the Steam Mobile App, or, without the app, with your password, by running the command the launcher shows in your own terminal. Fenji never sees your Steam login.
4. **Press Play.** On Linux the launcher starts the game through umu-launcher and the latest GE-Proton, which it keeps up to date itself, and under Feral GameMode where that's installed; Settings takes launch options as Steam does, for MangoHud and the like. On Windows and macOS, Play isn't available yet; the rest works.

The launcher updates itself (Settings, Auto-update), and it sends nothing about you; [PRIVACY.md](PRIVACY.md) lists what it fetches. The command-line `fenji-launch` does the same for scripts.

## Hosting

Anyone can host a Fenji server for the friends they invite, on their own computer or on a VPS, much as they would a Minecraft server: unpack the server's archive from a release (from v0.5.0), run its start script, type `invite` in its console, and send the link. Friends on your network can join at once. [docs/hosting.md](docs/hosting.md) is the guide for hosts: letting friends in over the internet or a VPN, the reference deployment on a VPS with systemd, updating, backups and troubleshooting.

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

The `archive` and `luatool` programs contain GPL-3.0-or-later code from `crates/fenji-oodle`, a port of [ooz](https://github.com/powzix/ooz)'s Oodle decoder, so its [COPYING](crates/fenji-oodle/COPYING) and [NOTICE](crates/fenji-oodle/NOTICE) go with any copy of them.
