# Fenji docs

| Path | What it holds |
|---|---|
| [`superpowers/specs/2026-10-11-fenji-design.md`](superpowers/specs/2026-10-11-fenji-design.md) | The project design: ground rules, architecture, milestones. Start here. |
| [`superpowers/specs/2026-10-12-m1-data-layer-design.md`](superpowers/specs/2026-10-12-m1-data-layer-design.md) | M1 design: Lua tooling, the login hunt and first connection, the script census, archives and the Oodle gate. |
| [`superpowers/specs/2026-10-12-m2-transport-design.md`](superpowers/specs/2026-10-12-m2-transport-design.md) | M2 design: the transport reading, `fenji-proto`, the stub's handshake ladder, the Oodle wire test and the archive helper. |
| [`superpowers/specs/2026-10-12-m3-login-lobby-design.md`](superpowers/specs/2026-10-12-m3-login-lobby-design.md) | M3 design: accounts and the account API, the gate and the lobby, the ticket handoff and `fenji-launch`. |
| [`superpowers/specs/2026-10-13-m4-social-sandbox-design.md`](superpowers/specs/2026-10-13-m4-social-sandbox-design.md) | M4 design: its three pieces in order (the world, hosting, the desktop launcher) and what they share. Each piece gets its own spec. |
| [`superpowers/specs/2026-10-13-m4-world-design.md`](superpowers/specs/2026-10-13-m4-world-design.md) | M4, part 1, the world: entering one shared scene, seeing each other, movement, chat, admin commands, and how the work runs. |
| [`superpowers/plans/2026-10-13-m4-world.md`](superpowers/plans/2026-10-13-m4-world.md) | The world's implementation plan: the cloud tasks, whose code was compiled and tested before the plan was written; the local readings and runs; and the amendments they add. |
| [`superpowers/specs/2026-10-14-m4-hosting-design.md`](superpowers/specs/2026-10-14-m4-hosting-design.md) | M4, part 2, hosting: the host console, maintenance mode, server facts, TLS without a domain, backups, release archives and the write-up for hosts. |
| [`superpowers/plans/2026-10-14-m4-hosting.md`](superpowers/plans/2026-10-14-m4-hosting.md) | The hosting piece's implementation plan: backups, the first run, server facts and maintenance, TLS with pins, the console and its front ends, attaching, release archives and the write-up. |
| [`superpowers/specs/2026-10-14-oodle-decoder-design.md`](superpowers/specs/2026-10-14-oodle-decoder-design.md) | The open Oodle decoder: `fenji-oodle`, a safe Rust port of `ooz`'s Kraken decoder (GPL-3.0-or-later), and how Fenji's tools read the game's archives with it. |
| [`superpowers/plans/2026-10-14-oodle-decoder.md`](superpowers/plans/2026-10-14-oodle-decoder.md) | The Oodle decoder's implementation plan. |
| `superpowers/plans/` | One implementation plan per milestone (M0–M5), or per piece where a milestone's spec splits it (M4), written before that work starts. |
| [`development.md`](development.md) | Machine setup (macOS/Linux), branches, Conventional Commits, SemVer, changelog, releases. |
| [`re/README.md`](re/README.md) | Reverse-engineering workflow: pinned build, analysis setup, sandboxed client runs, what may be committed. |
| [`re/sources.md`](re/sources.md) | External sources we used, and sources rejected for clean-room reasons. |
| [`re/pinned-build.md`](re/pinned-build.md) | The exact SoLO build Fenji targets (AppID, depot manifest, exe SHA-256). |
| [`re/spike-0.md`](re/spike-0.md) | M0 spike: static triage of the pinned client, runtime observation, conclusions. |
| [`re/m1-script-format.md`](re/m1-script-format.md) | M1: the client's embedded file system, blob format, Lua 5.3 dialect and extra opcodes. |
| [`re/m1-script-census.md`](re/m1-script-census.md) | M1: how much server-side logic the client's embedded scripts carry. |
| [`re/m1-archives.md`](re/m1-archives.md) | M1: the data archives' structure, read without decompressing Oodle data. |
| [`re/m1-summary.md`](re/m1-summary.md) | M1 summary: spike-0's questions answered, deviations, Done-when status. |
| [`re/m2-tables.md`](re/m2-tables.md) | M2: the client's table format, and where items, skills, monsters, the XP curve and maps live. |
| [`re/m2-summary.md`](re/m2-summary.md) | M2 summary: the transport answered, deviations, Done-when status. |
| [`re/m3-summary.md`](re/m3-summary.md) | M3 summary: the ticket and the players' path answered, deviations, Done-when status, the bare-metal runs. |
| [`protocol/README.md`](protocol/README.md) | Our protocol findings (transport, opcode ledger, payloads) and their format. |
| [`protocol/login.md`](protocol/login.md) | M1: how the client finds its server, and the first connection to our stub. |
| [`protocol/transport.md`](protocol/transport.md) | M2: the client's transport: hello, variants, frames and keys, the Oodle wrapper, auth. |
| [`protocol/packets/lobby.md`](protocol/packets/lobby.md) | M3: the lobby's script messages: the character list, creating and deleting characters, error codes and close reasons. |
| [`protocol/packets/world.md`](protocol/packets/world.md) | M4: entering the world: 772, the native types 3, 7, 8 and 23, the property blob, time syncs and the creation outfit; seeing each other: types 6, 5 and 9 and another player's 7; two clients on one machine; moving: types 8, 13 and 20, the move clock and the units. |

Ground rules (full text in the design spec, §2): legitimate copies only; clean room, no leaked server material; no game content in the repo; a patched or instrumented client only ever talks to Fenji; private and non-commercial.
