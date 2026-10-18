# M4: summary

M4, the social sandbox, turned the lobby into a world friends can share, and gave hosts and players what they need to get there. It came in three pieces:
- **The world.** Players enter the starting zone, see each other, move and chat, and admins' commands work.
- **Hosting.** A host runs a server with a console, an MOTD, maintenance mode, ops and bans, TLS without a domain and backups, from a release archive's start script or as a service on a VPS.
- **The desktop launcher.** A player redeems an invite link, signs in, checks their own copy of the game, downloads or repairs it through DepotDownloader, plays on Linux, and gets signed updates.

The owner designed M4 in conversation on 2026-10-13, as a short [umbrella spec](../superpowers/specs/2026-10-13-m4-social-sandbox-design.md) and then a spec and a plan for each piece, in the order world, hosting, launcher.

**Status (2026-10-16):** All three pieces are built, reviewed and merged into `dev`. Each had a final review of the whole piece, and every Critical or Important finding was fixed before the piece was called done. Hosting's was in two halves: the server's security and correctness, and the host's experience, with a first-time host's run of the write-up.

M4's acceptance (umbrella §5) is a session with friends on other machines, each with their own copy of the game. They meet in the starting zone through a hosted server and the desktop launcher, move, see each other and chat, and an admin's commands work. That session is the owner's to run, with the checks only real machines can make, such as a real VPS, a real Steam download, and Windows and macOS (#359). By the owner's decision of 2026-10-16, v0.5.0 ships before it, and what it finds ships in 0.5.x releases. Play is Linux-only until gate #337 is decided, so for now those friends need Linux.

## What M4 built

### The world

The [world spec](../superpowers/specs/2026-10-13-m4-world-design.md) and [plan](../superpowers/plans/2026-10-13-m4-world.md) ran as four vertical slices with real clients, and cloud tasks around them. Each slice read the client first, then built, then ran two clients against the server.

- **Entering** (slice 1, #217, #218): the client's Enter Game (10) gets the scene load (772), the client's ready (type 3), then its own character (7, 23 and 8), at full health, in its creation outfit. A new character starts at `[world]`'s start in scene 5; a returning one enters where it last stood. Character ids above 2³² work ([`world.md`](../protocol/packets/world.md) §1–§13).
- **Seeing each other** (slice 2, #240, #241): a 50 m interest grid. A player entering a cell is announced (6), each client pulls what it doesn't know (5) and gets its 7 and 8, and a player leaving is taken from view (9) (§14, §15). Two clients run in one sandbox, each in its own Wine desktop.
- **Moving** (slice 3, #261, #263): the client's moves (type 8) are decoded, checked against a lenient speed rule and relayed (13) within a tick; a refused move is corrected (20). Places are saved every five minutes and on leaving or stopping (§16).
- **Chat and admin commands** (slice 4, #273, #274): nearby, world and whispers, by name or by clicking a name, with the client's own limits enforced again and refusals the client shows (270). Admins and ops have `!tp`, `!kick`, `!announce` and `!invite` in chat ([`chat.md`](../protocol/packets/chat.md)). `!tp` to another scene was refused until a run checked its path; it works from 0.5.4 (#281; world.md 1.6).
- **Time syncs** are answered (#91).
- **The store** keeps where characters stand (#206), and the world runs as a headless Bevy 0.20 ECS on its own thread (#210).
- **The protocol bot** plays the world in CI, and a load harness ran 20 bots in one cell (#277, #278): the tick's p99 was 117–148 µs against a 10 ms target, and a shutdown with all 20 in took 64–66 ms against 2 s (world.md §13).
- **Never sent:** the messages that could write files, open pages, replace scripts or run code on a player's computer are on a list the server refuses to build ([`lobby.md`](../protocol/packets/lobby.md) §2).

The world's final review found one Important issue, fixed in #283; its follow-ups are #281 and #282.

### Hosting

The [hosting spec](../superpowers/specs/2026-10-14-m4-hosting-design.md) and [plan](../superpowers/plans/2026-10-14-m4-hosting.md), with Amendment A (the owner, 2026-10-15): hosting and joining as easy as a Minecraft server.

- **Backups** (#271): `backup`, a nightly timer on the reference VPS, 14 kept, each checked before older ones are pruned.
- **The first run** (#275): with no settings, the start script writes a LAN-ready `fenji.toml` and starts; `init` writes one without starting.
- **Server facts** (#279): the server's name, host, MOTD, state, version and who is online, in the account API, which the launcher shows. Maintenance mode keeps players out, admins excepted, and tells them why. Ops and bans live in the database and act at once.
- **TLS without a domain** (#286, #309): `tls = "self"` serves the account API with the server's own key, and invite links carry its pin, which the launcher checks. Hosts with a domain can use Caddy instead. Invite links come one per address the server has (HA4).
- **The host console** (#147; #319, #327, #330, #342, #346, #350): a full-screen console with the log above and a command line below, or plain lines with `--plain`. Its commands run from `invite` and `online` to `maintenance`, `motd`, `op`, `ban` and `stop`. `fenji-server console` attaches to a running server through a socket only the server's user can reach, and one server holds a data folder at a time. Players' text is shown with its hidden characters as escapes everywhere it reaches the host.
- **Release archives** (#321, #343): the server for Linux (x86_64 and arm64), Windows and macOS, each with a start script, the example settings, a README and its third-party notices.
- **The write-up for hosts** (#351): [`docs/hosting.md`](../hosting.md), from running the start script on your own computer to a VPS with systemd, Caddy, a firewall and backups. It was verified by following it literally in a fresh Debian 13 container with systemd.
- **The final review's fixes** (#356, #358):
  - closing the terminal or the SSH session stops a server cleanly, unless `nohup` keeps it running;
  - the console prints the server link that needs no invite, with `invite`, `link` and at the start;
  - the data folder must be the server's user's;
  - `console --run stop` waits for the stop;
  - a backup never passes a partial copy;
  - links copied from a wrapped console row still work in both launchers;
  - the errors a host meets say what to do.

### The desktop launcher

The [launcher spec](../superpowers/specs/2026-10-15-m4-launcher-design.md) and [plan](../superpowers/plans/2026-10-15-m4-launcher.md), built inline in the design the owner approved on 2026-10-13 ([`DESIGN.md`](../../DESIGN.md)).

- **The shell** (#313–#324): Tauri 2, with the state and every secret in the Rust core and the view only drawing snapshots (L1).
- **Servers and accounts:** invite links with pins, accounts per server, and each server's state (online, maintenance, offline, or a version it can't play).
- **The player's copy:** a quick check by size and a full check by hash against the pinned build's 210 files, and download and repair through DepotDownloader, where the player signs in to Steam in DepotDownloader's own window (L3).
- **Play on Linux** through umu-run. Windows and macOS builds ship with Play disabled (L2, gate #337).
- **Signed self-updates** (#340, #347): the release key lives only in a `release` environment that admits `v*` tags and waits for the owner's approval (#352).
- **The bundles:** an AppImage, a Windows installer and a macOS app, with their third-party notices (#345).
- **The end-to-end run on bare metal** (#339): a new player went from an invite link to character creation, under every guard ([`m4-launcher-e2e.md`](m4-launcher-e2e.md)).

The launcher's final review found two Important issues, fixed in #347; its deferred minors are #348.

### Alongside

- **Client patching** (#225, #226): the owner allowed patching the client, as long as it only ever talks to Fenji (ground rule 6). Patches ship after M4 (#336); M4 runs the unmodified client.
- **The Oodle decoder** (#314, #317, #284): every Oodle block in the pinned client decodes natively, matching Epic's library.
- **Sandboxed runs on the GPU** (#266–#268), which cut a scene load from about 50 s to under 10 s.
- **The runners and the watchdog** (#207, #215; #323, #354): hardening, and two flaky self-test checks fixed.

## The documents

- [`../protocol/packets/world.md`](../protocol/packets/world.md): entering, the player's own actor, time syncs, the creation outfit, seeing each other, two clients on one machine, moving, the runs and the 20-bot measurement.
- [`../protocol/packets/chat.md`](../protocol/packets/chat.md): saying and hearing, the channels Fenji uses, refusals, the admin commands and the runs with two real clients.
- [`m4-launcher-e2e.md`](m4-launcher-e2e.md): the launcher's end-to-end run on bare metal, and what its rehearsals found.
- [`../hosting.md`](../hosting.md): the write-up for hosts.
- [`../../DESIGN.md`](../../DESIGN.md): the brand and the launcher's design.
- README, PRIVACY.md and SECURITY.md: what M4 changes for players and hosts.

## Open questions answered (umbrella §6)

| Question | Answer | Where |
|---|---|---|
| The starting scene and its spawn point | Scene 5, at `[world]`'s start; every vocation tried starts there and stands in colour at full health | world.md §13 |
| The world's messages | Read, built and run: entering, seeing, leaving, moving, correcting and chat | world.md, chat.md |
| One connection or two | One: the game connection to the gate carries the world | world.md §1, §13 |
| Where the outfit comes from | The creation outfit the client chose, stored with the character | world.md §9 |
| TLS without a domain | `tls = "self"`, the server's own key, pinned in its links (H1) | hosting spec §3.10 |
| An open game port or a VPN | Open ports by default; the write-up says plainly what the game connection exposes, and recommends a VPN for privacy (H2) | hosting.md §3 |
| Packaging for Windows and macOS | Archives with start scripts; no vendor signing (H3) | hosting spec §3.7 |
| The signing key and the channel | A key made on the owner's machine, in a tag-only environment that waits for the owner; updates from the latest GitHub release (L5) | launcher spec §3.8 |
| Windows and macOS while Play is Linux-only | They manage servers, accounts and copies, with Play disabled and the reason shown (L2) | launcher spec §3.7 |

## Done when (umbrella §3)

| Piece | Item | Status | Evidence |
|---|---|---|---|
| World | A new character enters the starting zone; a returning one enters where it left | **Met** | m4-e1–e4, m4-m2 |
| World | Movement passes the speed check, and positions persist | **Met** | m4-m1b–m4-m4 |
| World | Players see each other within 50 m, with names, appearance and outfit, and every change updates everyone | **Met** | m4-s4, m4-s5, m4-m1b |
| World | Chat nearby, server-wide and as whispers | **Met** | t16-c1–c3 |
| World | Time syncs answered (#91) | **Met** | world.md §7 |
| World | Admins' `!tp`, `!kick`, `!announce` and `!invite` | **Met**, `!tp` within a scene | t16-c1, t16-c3; #281 |
| World | The protocol bot in CI, and 20 bots measured | **Met** | #277, #278 |
| World | Two real clients meet in the zone | **Met** | m4-s4 onward |
| Hosting | The reference deployment works by following the write-up alone | **Met in a container**; a real VPS is the acceptance session's | #351 |
| Hosting | Hosts without a domain have a documented path | **Met** | hosting.md, `tls = "self"` |
| Hosting | The host console on Linux, Windows and macOS, over SSH, with its commands, a first run and start scripts | **Met** on Linux; Windows and macOS read, built and packed, not run | #319–#350, #321 |
| Hosting | Maintenance reaches players | **Met** | #279, #342 |
| Launcher | Redeem, sign in, and check the copy quick and full | **Met** | m4-launcher-e2e |
| Launcher | Download and repair through DepotDownloader | **Built and tested against stand-ins;** a real download needs the owner's Steam sign-in, so it goes with the acceptance session | the launcher plan's Task 9 |
| Launcher | Play on Linux | **Met** | m4-launcher-e2e |
| Launcher | The server's states | **Met:** the model's probes have their tests, and each state's screen was checked against its board | the launcher's tests; the finish review's pairs |
| Launcher | Updates from signed releases | **Met** in a local test (a test update installed, a tampered one refused) | #340 |
| M4 | Friends on other machines meet in the starting zone | **Pending: the owner's session** | — |

## Gates and the owner's rulings

| Gate | Outcome |
|---|---|
| #225: patching the client | **Decided 2026-10-14:** allowed, as long as a patched client only ever talks to Fenji. Patches ship as data the launcher applies, after M4 (#336). |
| #337: Play on native Windows, and the AGT driver | **Open,** the owner's. |
| #338: a proper English translation | **Open,** the owner's. |
| #11: the mocked SDK | Unchanged: sandboxed developer runs only. |
| #114: the launcher path | Unchanged: players use it; guarded bare-metal tests on the Linux game box only. |

The watchdog saw no sign of the AGT driver in any M4 run.

The owner's rulings during M4:
- **The structure** (2026-10-13): an umbrella spec, then a spec and a plan per piece, in the order world, hosting, launcher.
- **The world's engine** (2026-10-13): the newest stable Bevy major, 0.20.
- **Cloud agents** (2026-10-13): repository-only tasks, on Opus at max effort; client-derived material never goes to the cloud. At most two subagents at once (2026-10-14).
- **Hosting as easy as a Minecraft server** (2026-10-15): Amendment A, HA1–HA6.
- **The launcher built inline** (2026-10-15), as ordinary engineering, with one review at the end.
- **Releases pre-approved** (2026-10-15), and **v0.5.0 before the acceptance session**, with what it finds in 0.5.x releases (2026-10-16).
- **The release environment** (2026-10-16): the owner is its required reviewer, so a pushed tag alone can't sign an update.

## Findings M5 inherits

- **The world:** `!tp` across scenes (#281), back to character select (#282), and the rest of #281's follow-ups. Each [world.md](../protocol/packets/world.md) section ends with its unknowns, such as health over another player's head (M5's combat).
- **Known M4 limitations** in chat (#281):
  - **Item and player links in chat are refused.** The client's input turns a link the player adds into `[link …]` markup ([chat.md](../protocol/packets/chat.md) 1.5), and the world refuses every line with markup, so such a line gets "The message cannot be sent" (chat.md §5). A typed line is refused only when it holds a backslash, or a `[` whose next character, past any spaces, isn't a digit or `]`, or that ends the line, spaces aside.
  - **A stray backslash in a server line with a `]` and no `[`.** `server_text` writes a `\` before each bracket, which the client's tag parser reads as the bracket itself, but the parser runs only on a line that holds a `[` (chat.md 2.5). So a lone `]` shows as `\]`. It's cosmetic, and only the server's own system lines can carry one: the answers to admins' and the console's commands, and the MOTD the host sets.
- **Hosting:** the reviews' follow-ups and the acceptance session's checks are in #359. The Windows and macOS steps have been read, built and packed, not run.
- **The launcher:** the deferred minors in #348; Windows and macOS Play wait on gate #337.
- **Patching:** the first patch set (#336), starting with cutting every way the client can reach anything but Fenji. Since 2026-10-18 it ships in 0.6.0 with the retranslation, before M5 ([`2026-10-18-retranslation-design.md`](../superpowers/specs/2026-10-18-retranslation-design.md)).
- **The runners:** #244 and #174's last two items.
