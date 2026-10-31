# Fenji docs

Playing on a friend's server needs only the desktop launcher: the root [README](../README.md#playing) says how to get it and use it, and [PRIVACY.md](../PRIVACY.md) what it sends. Hosting a server for your friends is in [`hosting.md`](hosting.md). The other documents below are for people working on Fenji.

| Path | What it holds |
|---|---|
| [`hosting.md`](hosting.md) | For hosts: running a Fenji server for invited friends on your own computer or a VPS, the console and its commands, letting friends in, the reference deployment with systemd, updating, backups and troubleshooting. |
| [`superpowers/specs/2026-10-11-fenji-design.md`](superpowers/specs/2026-10-11-fenji-design.md) | The project design: ground rules, architecture, milestones. Start here. |
| [`superpowers/specs/2026-10-12-m1-data-layer-design.md`](superpowers/specs/2026-10-12-m1-data-layer-design.md) | M1 design: Lua tooling, the login hunt and first connection, the script census, archives and the Oodle gate. |
| [`superpowers/specs/2026-10-12-m2-transport-design.md`](superpowers/specs/2026-10-12-m2-transport-design.md) | M2 design: the transport reading, `fenji-proto`, the stub's handshake ladder, the Oodle wire test and the archive helper. |
| [`superpowers/specs/2026-10-12-m3-login-lobby-design.md`](superpowers/specs/2026-10-12-m3-login-lobby-design.md) | M3 design: accounts and the account API, the gate and the lobby, the ticket handoff and `fenji-launch`. |
| [`superpowers/specs/2026-10-13-m4-social-sandbox-design.md`](superpowers/specs/2026-10-13-m4-social-sandbox-design.md) | M4 design: its three pieces in order (the world, hosting, the desktop launcher) and what they share. Each piece gets its own spec. |
| [`superpowers/specs/2026-10-13-m4-world-design.md`](superpowers/specs/2026-10-13-m4-world-design.md) | M4, part 1, the world: entering one shared scene, seeing each other, movement, chat, admin commands, and how the work runs. |
| [`superpowers/plans/2026-10-13-m4-world.md`](superpowers/plans/2026-10-13-m4-world.md) | The world's implementation plan: the cloud tasks, whose code was compiled and tested before the plan was written; the local readings and runs; and the amendments they add. |
| [`superpowers/specs/2026-10-14-m4-hosting-design.md`](superpowers/specs/2026-10-14-m4-hosting-design.md) | M4, part 2, hosting: the host console, maintenance mode, server facts, TLS without a domain, backups, release archives and the write-up for hosts. |
| [`superpowers/plans/2026-10-14-m4-hosting.md`](superpowers/plans/2026-10-14-m4-hosting.md) | The hosting piece's implementation plan: backups, the first run, server facts and maintenance, TLS with pins, the console and its front ends, attaching, release archives and the write-up. |
| [`superpowers/specs/2026-10-15-m4-launcher-design.md`](superpowers/specs/2026-10-15-m4-launcher-design.md) | M4, part 3, the desktop launcher: the server list, accounts, the player's copy (checks, and downloads and repairs through DepotDownloader), Play on Linux and signed self-updates. |
| [`superpowers/plans/2026-10-15-m4-launcher.md`](superpowers/plans/2026-10-15-m4-launcher.md) | The launcher's implementation plan: the model in `fenji-launcher`, the Tauri shell, the view's screens, updates and releases, the end-to-end run and the finish review. |
| [`design/launcher/`](design/launcher/README.md) | The launcher's approved boards (2026-10-13), one per state, with the host console and the brand tokens; [`DESIGN.md`](../DESIGN.md) is the system they share. |
| [`superpowers/specs/2026-10-14-oodle-decoder-design.md`](superpowers/specs/2026-10-14-oodle-decoder-design.md) | The open Oodle decoder: `fenji-oodle`, a safe Rust port of `ooz`'s Kraken decoder (GPL-3.0-or-later), and how Fenji's tools read the game's archives with it. |
| [`superpowers/plans/2026-10-14-oodle-decoder.md`](superpowers/plans/2026-10-14-oodle-decoder.md) | The Oodle decoder's implementation plan. |
| [`superpowers/specs/2026-10-18-retranslation-design.md`](superpowers/specs/2026-10-18-retranslation-design.md) | The retranslation: every line of the game's English translated anew from its Chinese, kept in the separate `fenji-content` repository and applied at launch; gate #338's answers and ground rule 3's amendment. |
| [`superpowers/plans/2026-10-18-retranslation.md`](superpowers/plans/2026-10-18-retranslation.md) | The retranslation's implementation plan: `fenji-pack`, the `translate` tool (extract, checks, glossary, batches, the driver of headless runs, assemble), the client's locale and the early check, `fenji-content`, the bulk, and the launcher's part for 0.6.0. |
| `superpowers/plans/` | One implementation plan per milestone (M0–M5), or per piece where a milestone's spec splits it (M4), written before that work starts. |
| [`development.md`](development.md) | Machine setup (macOS/Linux), branches, Conventional Commits, SemVer, changelog, releases, and content packs and their key. |
| [`re/README.md`](re/README.md) | Reverse-engineering workflow: pinned build, analysis setup, sandboxed client runs, what may be committed. |
| [`re/sources.md`](re/sources.md) | External sources we used, and sources rejected for clean-room reasons. |
| [`re/pinned-build.md`](re/pinned-build.md) | The exact SoLO build Fenji targets (AppID, depot manifest, exe SHA-256). |
| [`re/spike-0.md`](re/spike-0.md) | M0 spike: static triage of the pinned client, runtime observation, conclusions. |
| [`re/m1-script-format.md`](re/m1-script-format.md) | M1: the client's embedded file system, blob format, Lua 5.3 dialect and extra opcodes. |
| [`re/m1-script-census.md`](re/m1-script-census.md) | M1: how much server-side logic the client's embedded scripts carry. |
| [`re/m1-archives.md`](re/m1-archives.md) | M1: the data archives' structure, `_index` and the stored files; since then, the Oodle census, and the check that Fenji's own decoder reads every Oodle block as the client's DLL does. |
| [`re/m1-summary.md`](re/m1-summary.md) | M1 summary: spike-0's questions answered, deviations, Done-when status. |
| [`re/m2-tables.md`](re/m2-tables.md) | M2: the client's table format, and where items, skills, monsters, the XP curve and maps live. |
| [`re/m2-summary.md`](re/m2-summary.md) | M2 summary: the transport answered, deviations, Done-when status. |
| [`re/m3-summary.md`](re/m3-summary.md) | M3 summary: the ticket and the players' path answered, deviations, Done-when status, the bare-metal runs. |
| [`re/m4-summary.md`](re/m4-summary.md) | M4 summary: the world, hosting and the desktop launcher, the open questions answered, Done-when status, gates and rulings, and what M5 inherits. |
| [`re/m4-launcher-e2e.md`](re/m4-launcher-e2e.md) | M4: the desktop launcher's end-to-end run on bare metal, from an invite link to the lobby, and what its rehearsals found. |
| [`re/retranslation-locale.md`](re/retranslation-locale.md) | The retranslation: how the client loads its locale cache, the language Fenji's start gives it, the engine's file system with its patch parts and `--fs` indexes, and the early check's loader. |
| [`protocol/README.md`](protocol/README.md) | Our protocol findings (transport, opcode ledger, payloads) and their format. |
| [`protocol/login.md`](protocol/login.md) | M1: how the client finds its server, and the first connection to our stub. |
| [`protocol/transport.md`](protocol/transport.md) | M2: the client's transport: hello, variants, frames and keys, the Oodle wrapper, auth. |
| [`protocol/packets/lobby.md`](protocol/packets/lobby.md) | M3: the lobby's script messages: the character list, creating and deleting characters, error codes and close reasons. |
| [`protocol/packets/world.md`](protocol/packets/world.md) | M4: entering the world: 772, the native types 3, 7, 8 and 23, the property blob, time syncs and the creation outfit; seeing each other: types 6, 5 and 9 and another player's 7; two clients on one machine; moving: types 8, 13 and 20, the move clock and the units; the runs, and the 20-bot load measurement. |
| [`protocol/packets/chat.md`](protocol/packets/chat.md) | M4: chat: the line a player says (115) and the line a client shows (378), the player link, the channels Fenji uses, refusals (270) and their codes, the admins' commands, and the runs with two real clients. |

Ground rules (full text in the design spec, §2): legitimate copies only; clean room, no leaked server material; no game content in the repo; a patched or instrumented client only ever talks to Fenji; never monetised.
