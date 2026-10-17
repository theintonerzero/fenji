# M4: Social sandbox + VPS — Design Spec

**Status:** designed with the owner in conversation on 2026-10-13, decision by decision and section by section, and written up here for the owner's review (#176).

**Parent:** [Fenji design spec](2026-10-11-fenji-design.md):
- §6.4 (the server), §6.6 (admin) and §6.7 (capacity);
- §8 (deployment), §9 (testing) and §10 (M4).

M3's findings are in [`docs/re/m3-summary.md`](../../re/m3-summary.md), under "Findings M4 inherits". The issues are #146 (the desktop launcher), #147 (the host console), #91 (time sync) and #174 (v0.4.1's follow-ups).

## 1. Purpose

M4 is v0.5.0. It ends when friends a host invited meet in the starting zone through a hosted server, each with their own copy of the game. They move, see each other and chat, and the host's admins have commands in chat.

The parent spec's bar for M4 is "friends enter the starting zone, move, see each other and chat; admin commands; the reference deployment, written up so anyone can host their own server". On 2026-10-13 the owner added the desktop launcher (#146) and a console for hosts (#147).

**How M4 is planned (the owner's decision, 2026-10-13):**
- M4 is several times M3's size, and its pieces can be built and tested apart. This spec fixes its scope, the order of its pieces and what they share.
- Each piece then gets its own spec and plan, brainstormed when its turn comes, and merges into `dev` as it's done.
- v0.5.0 ships when all three are in, under the usual release rules.
- For M4, this replaces "one implementation plan per milestone".

## 2. Constraints

Unchanged from the parent spec and CLAUDE.md:
- **The ground rules:**
  - legitimate copies only;
  - the clean room;
  - no game content in the repository;
  - client runs only through the runners;
  - decision gates stop work;
  - Fenji never handles platform credentials.
- **The real, unmodified client.** Play is Linux-only until the owner decides the AGT driver question on native Windows (#114).
- **Size:** a server is for a host's invited friends, 20 players by design (parent §6.7).

## 3. The pieces, in order

The owner chose the order (2026-10-13): the world, then hosting, then the desktop launcher.
- **The world** is the heart of M4 and its largest unknown, so any wall or decision gate in it shows up before anything is built around it.
- **Hosting** then lets friends on other machines join the world's tests.
- **The launcher** comes last. Its design is approved, and its server states read what hosting defines, which §4 fixes now.

### 3.1 The world

Done when:
- A new character enters the starting zone, and a returning one enters where it left.
- Movement passes the parent spec's lenient speed check (§6.4), and positions persist across sessions.
- Players see each other within the parent spec's 50 m interest grid, with names, appearance and outfit. Joining, leaving, moving and going out of range update everyone.
- Chat works nearby, across the whole server, and as whispers.
- The server answers the client's time syncs (#91).
- Admins (`[access].admins`) get `!tp`, `!kick`, `!announce` and `!invite` in chat, from the command set of §4.3. They use `!` because the client keeps lines starting with `/` to itself ([world spec](2026-10-13-m4-world-design.md) §2.4).
- A protocol bot built on `fenji-proto` (sign in, enter, move, chat) runs in CI (parent §9), and 20 bots measure the tick against the parent spec's targets (§6.7).
- Two real clients on the owner's machine meet in the zone. Each runner gives its client a loopback-only network of its own, so both clients run in one runner's namespace, against its one server ([world spec](2026-10-13-m4-world-design.md) §4.3).

**Status (2026-10-17):** done. `!tp` to another scene, refused until a run checked it, works since 0.5.4 (#281; world.md 1.6). See [`m4-summary.md`](../../re/m4-summary.md).

**Out of M4:**
- emotes, teams, friends lists, mounts, NPCs and quests;
- everything M5 owns: monsters, combat, items and inventory, and the `/give`, `/spawn`, `/rate` and `/level` commands.

**What M3 leaves it** ([`lobby.md`](../../protocol/packets/lobby.md) §10, [`m3-summary.md`](../../re/m3-summary.md)):
- **Entering.** The client asks to enter with message 10 and a character's pid, and M3 refuses with 270. A world may first queue the player (605, 728), then loads the scene with 772 (**Inferred**).
- **Connections.** Whether the world uses the gate's connection or a second one is the parent spec's last open transport question (§12).
- **Time sync.** The reply is in [`transport.md`](../../protocol/transport.md) 10.6. One correction comes first, from the script itself (#91).
- **Outfits.** M3 sends `prop_sys_equips = {}` (ruling P4). The outfit chosen at creation comes from the client's own tables in the operator's install, and the world's spec decides how.

This piece is mostly reverse engineering of the client's world messages, as M1–M3 were: static reading first, evidence in `docs/protocol/`, then sandboxed runs. If anything would mean patching the client or loading its driver, work stops for the owner's decision (ground rule 6).

**#174's items that belong here:**
- a replaced connection's close message, sent while outside the caps (the gate);
- the runner and watchdog items, which the world's client runs exercise.

### 3.2 Hosting

Done when:
- The reference deployment of parent §8 works on a fresh VPS by following the write-up alone: systemd, Caddy for the account API's TLS, the firewall and nightly backups.
- Hosts without a domain have a documented path.
- The host console (#147) runs `fenji-server` with live logs above and a command line below:
  - on Linux, Windows and macOS, and over SSH;
  - with §4.3's commands, plus `online`, `accounts`, `maintenance` and `stop`;
  - a first run without a config writes one, and start scripts ship in the release archives.
- Maintenance mode (§4.2) reaches players.

**Status (2026-10-16):** done. The write-up was followed literally in a fresh Debian 13 container with systemd, and a real VPS is the acceptance session's (§5). The console was run on Linux; on Windows and macOS it was built, packed and read, not run (#359). See [`m4-summary.md`](../../re/m4-summary.md).

**Decisions for its spec:**
- How hosts without a domain get TLS for the account API.
- Whether play over the internet uses an open game port or a VPN, and what the write-up tells hosts. The game connection authenticates nobody and hides nothing ([`transport.md`](../../protocol/transport.md) 3.9).

**#174's items that belong here:**
- the description of `public_host`;
- the invite link's fallback to `api_bind` when the gate is public.

### 3.3 The desktop launcher

Done when #146's brief is built in the design the owner approved on 2026-10-13. Its `DESIGN.md` comes with #146.
- Redeem an invite, sign in, and verify the client, quick and full, file by file.
- Download and repair through DepotDownloader for Steam owners, who sign in to Steam in DepotDownloader's own window, never in Fenji. Owners of a Gameforge copy point to their folder.
- Play on Linux.
- Show the server's states, online, maintenance and offline, from §4.1.
- Update itself from signed releases.

**Status (2026-10-16):** done. Download and repair were tested against stand-ins for DepotDownloader; a real Steam download needs the owner's sign-in, so it goes with the acceptance session (#359). See [`m4-summary.md`](../../re/m4-summary.md).

**Decisions for its spec:**
- the signing key and the update channel, which are the owner's to choose;
- what the launcher does on Windows and macOS while play is Linux-only.

### 3.4 v0.4.1's follow-ups (#174)

Each item goes into the piece that touches its code, as §3.1 and §3.2 list. The docs nits go into whichever pull request edits the file.

## 4. What the pieces share

These are fixed here, so that no piece waits on another.

### 4.1 Server info

The account API answers `GET /api/v1/server` to anyone, with:
- the server's name and Fenji's version;
- the client build it expects (the pinned build);
- the number of players online, and the cap;
- its state: `online`, or `maintenance` with the host's message and an optional end time.

A server that doesn't answer is offline; the launcher concludes that itself. The approved launcher design shows who is online. That list goes only to signed-in players, and PRIVACY.md says so.

### 4.2 Maintenance mode

Turning maintenance mode on:
- stops new sign-ins and game tickets, with the host's message, which §4.1 carries and the launcher shows;
- announces it in the game.

Players already in stay until the host kicks them or stops the server.

### 4.3 One admin command set

One module in `fenji-server`, called by both the game's chat (for `[access].admins`) and the host console, with the same names and arguments.
- The world piece builds it with `!tp`, `!kick`, `!announce` and `!invite`. In chat they start with `!`, since the client keeps `/` lines to itself; the console takes them without the `!`.
- Hosting adds the console's own commands, `online`, `accounts`, `maintenance` and `stop`, which only the console has.

### 4.4 Addresses

Players' launchers and invite links use `[server] public_api` (https; #154). The game connection keeps `public_host` and the gate's port. How hosts without a domain get an https address is hosting's decision (§3.2).

### 4.5 Releases

One version for the whole workspace, as now, and v0.5.0 is the milestone's release (parent §10). The release archives gain:
- the server for Linux, Windows and macOS, with its start scripts (hosting);
- the launcher's signed installers (the launcher).

### 4.6 Tests across pieces

The protocol bot is both the world's integration test and its load test (§3.1). M4's final acceptance (§5) is a session with friends on other machines.

## 5. Acceptance

M4 is done when friends on other machines meet in the starting zone through a hosted server and the desktop launcher, each with their own copy of the game. They move, see each other and chat, and an admin's commands work.

The session is written up in `docs/re/m4-summary.md` with each piece's results. Then v0.5.0 ships.

## 6. Open questions

The pieces' specs and runs answer these, not guessing:
- **The world:**
  - the starting scene and its spawn point;
  - the world's messages: queueing, loading the scene, the player's own character, other players entering, leaving and updating, movement and chat;
  - one connection or two;
  - where the outfit comes from.

  The [world spec](2026-10-13-m4-world-design.md) settles how two real clients reach one server (§4.3), and lists its own open questions (§6).
- **Hosting:** TLS without a domain; an open game port or a VPN; packaging for Windows and macOS.
- **The launcher:** the signing key and channel; Windows and macOS while play is Linux-only.
