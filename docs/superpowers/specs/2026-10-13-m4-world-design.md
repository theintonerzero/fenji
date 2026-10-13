# M4, part 1: The world — Design Spec

**Status:** designed with the owner on 2026-10-13. The owner took every recommendation ("do all as you recommend"); this is the written form for the owner's review (#178).

**Parents:**
- the [M4 spec](2026-10-13-m4-social-sandbox-design.md), §3.1 and §4;
- the [Fenji design spec](2026-10-11-fenji-design.md): §6.4 (the server), §6.6 (admin), §6.7 (capacity) and §9 (testing).

M3's lobby is in [`lobby.md`](../../protocol/packets/lobby.md). The native message names are in [`opcodes.md`](../../protocol/opcodes.md).

## 1. Purpose

The world piece makes M4's social sandbox: characters enter one shared scene, move, see each other and chat, and admins have commands. Its bar is the M4 spec's §3.1.

It ends with two real clients meeting in the scene on the owner's machine, a protocol bot in CI and a 20-bot measurement of the tick. Hosting and the desktop launcher follow it (M4 spec §3).

## 2. What we know

These facts come from a read-only survey of the client's scripts and design tables, made while designing this piece. The notes are in the git-ignored `re/m4/world-survey/`.

Everything here is **Inferred** from scripts and tables unless marked otherwise. The plan's first tasks write each fact up with its evidence in `docs/protocol/`, and reading the exe settles the native formats.

### 2.1 Entering

- **Queueing is optional.** After 10, the select screen's request to enter with a pid, a server may queue the player (605, then 728). Nothing waits for either.
- **772 starts the load.** `PROTOS2C_ACTOR_LOAD_SCENE` carries:
  - the actor id;
  - a scene table: `scene_uid`, `scene_tid` and `scene_lv` are required, and `scene_type`, `is_pvp`, `is_switch` and `is_load` are optional;
  - a position `{x, y, z, yaw}`;
  - a version constant, 4706187382722034291, which the client compares with its own property schema. A mismatch only logs.
- **The client loads the map** named by its `maps` table row. Then it sends the native `CTS_PROTOCOL_CLIENT_ENTER` (type 3, a 21-byte body built in native code).
- **Only then may the server send the player's own actor:** the native `STC_PROTOCOL_SYNC_ENTER` (type 7). It carries a property blob, the actor id, the actor's type (1 for a player), a difficulty and a template id. The client creates the actor and leaves the loading screen. Sent before type 3, it leaves the client on the loading screen.
- **Then the finish notice.** `STC_PROTOCOL_SYNC_ENTER_FINISH` (type 8, with the actor id) re-enables the client's in-world script messages. The client holds them in a cache until then.
- **Time syncs start right after 772,** not once the actor exists. `transport.md` 10.6 says otherwise and gets corrected (#91).
- **The actor id is the character's pid.** It must be above 0, and for players below 2^48: the client treats ids with any of their top 16 bits set as NPCs.

### 2.2 Seeing each other

- **Two phases.**
  - The server announces an actor with the native `SYNC_ID_ENTER` (type 6): its id, position, a fast flag and a priority.
  - The client queues it and pulls each actor in with `CTS_PROTOCOL_SYNC_FILTER_SET` (type 5).
  - The server then sends the actor with type 7.
- **Updates and leaving.** Updates are type 10 (a property blob and the id). Leaving is type 9 (a list of ids).
- **Order.** Other actors announced before the player's own type 7 are dropped.
- **Properties.** There are 2,439 named properties, indexed in the order of the client's property table: sync class `all` first, then `self`, then the rest. The property blob's byte format is native and **Unknown**.
- **What another player needs:**
  - to be shown at all: position, vocation, skeleton, gender and name;
  - for its look: equips and face;
  - for its name board: level, title and guild.

### 2.3 Movement

- **Native from end to end.** Each input frame, the scripts call into the exe with the direction, the mode and the yaw. The exe sends the native `CTS_PROTOCOL_MOVE_*` messages, whose layouts and rates are **Unknown**.
- **The server's side.** It relays movement to others with `STC_PROTOCOL_MOVE_*` (types 12–19) and corrects a mover with `MOVE_SYNC_SVR` (type 20).
- **Walk and run** is script message 419, with one boolean. Jumps, dashes and glides are skill casts, probably sent as `CTS_PROTOCOL_MOVE_COMBAT` (type 9).
- **Speed is a rate.** `prop_move_speed` is a percentage: 100 is a normal run, below 60 is walking, and the cap is 340. Sprinting and gliding add 70%. A rate of 100 is about 500 cm/s.

  The rate sits outside the property stream and defaults to 0. So the server probably has to set it with the native `MOVE_BASE_PROP` (type 21) or `SYNC_PROP_FORCE` (type 23), or the character can't move.
- **Positions** are four numbers: x, y, z (up) and a yaw in degrees. The units are probably centimetres.

### 2.4 Chat

- **Sending.** Once in the world, the client sends 115 `PROTOC2S_CHAT_TRANSMIT` with a channel, the text and an extra value.
  - Of its 18 player channels, M4 needs 0 (nearby), 1 (whisper) and 5 (world).
  - A whisper's extra names the target by pid, or as `Name@Server`.
  - The client shows no local copy, so the server echoes each message to its sender.
- **Receiving.** The client shows chat from 378 `PROTOS2C_SYS_CHAT_MESSAGE`: the channel, the sender, the content, the receiver, options and a template. Sender and receiver are player links. The system channels include 52 (notice) and 66 (marquee).
- **Limits.** The client allows 140 characters, and 2048 bytes after escaping. The server has its own `ERR_CHAT_*` refusal codes (270), among them "too fast".
- **Lines starting with `/` never leave the client.** It handles them locally and silently drops words it doesn't know.

### 2.5 Where characters start, and their outfit

- **Start points.** The game's own tables give each vocation one:
  - vocations 0–5 start in scene 5;
  - vocations 6, 7 and 9 start in scene 3006;
  - the hidden vocation 8 starts in scene 3007, whose map the client doesn't ship.
- **Level.** Characters are created at level 37 (`CreateLv`), and their starting weapons require it. M3 creates them at level 1.
- **Outfit.** The creation outfit is `PreviewEquips[vocation][2]` from the client's tables. The create request always carries `prop_base_equips_id = 2`, and the head is hidden. An equips table maps slot numbers to item ids.

### 2.6 Messages a server must never send

- **68 `PROTOS2C_ROBOT_CMD`** makes the client write files at paths the server chooses, then run a script from disk.
- **714 `PROTOS2C_DESIGN_REMOTE_INVOKE`** calls any exported function, opening URLs among them.

Neither is on fenji-proto's never-send list yet.

## 3. Design

### 3.1 Entering

- **The sequence.** On 10, the server sends 772 with the character's scene and position. Once the client reports type 3, it sends:
  - the player's own actor (type 7);
  - its speed, with type 21 or 23, whichever the exe reading shows;
  - then type 8.

  Queueing isn't used.
- **The scene.** A new character enters the shared meeting scene, set in fenji.toml as `[world] start_scene` with `start_position` (x, y, z, yaw).
  - The default is a scene the world's runs verify for every vocation: its map ships with the client, and it loads and can be walked without the original server's scene scripts.
  - The game's per-vocation start points (§2.5) return with M5's content.
  - A returning character enters where it left.
- **One instance.** Everyone in the shared scene is in the same instance.

### 3.2 Characters

- **Creation.** New characters are created at level 37, in their vocation's creation outfit with the head hidden. The lobby shows the same level and outfit.
- **The outfit's source** (corrected by the [world plan](../plans/2026-10-13-m4-world.md), by the owner's decision of 2026-10-13, #180).
  - A host's server can't read the client's design tables, which are Oodle-compressed in the archives.
  - So the outfit comes from a table of item ids that Fenji ships, documented with evidence in `docs/protocol/`.
  - If the exe reading shows the client fills the outfit in itself from `prop_base_equips_id`, Fenji sends that and ships no ids.
- **Storage.** The store gains each character's scene and position. The outfit follows from the vocation and the outfit choice already stored, until M5 gives characters their own equipment. When the migration runs, M3's characters become level 37.
- **Saving.** Positions are saved when a character leaves, and every 5 minutes.

### 3.3 Seeing each other

- **Interest.** As the parent spec's interest management says: a 50 m grid (5,000 units, if centimetres hold), and each player sees their own cell and the eight around it.
- **The flow.**
  - A player coming into view is announced (type 6).
  - On the client's pull (type 5), the server sends them (type 7).
  - Their property changes go out as type 10.
  - Leaving the view, or the game, is type 9.
- **Properties in M4.** A player carries position, vocation, skeleton, gender, name, level, face, equips and the show flags. Everything else keeps the client's default.

### 3.4 Movement

- **The check.** The server checks each move against the mover's speed, with the parent spec's lenient tolerance (`max_speed × dt × tolerance`, the tolerance set in fenji.toml).
- **Results.** Good moves are relayed to whoever sees the mover. Bad ones get a correction (type 20).
- **Speed.** Every player's speed rate is 100 in M4: there are no buffs or mounts.

### 3.5 Chat

- **Routing.** 115 comes in and 378 goes out, each message echoed to its sender:
  - nearby: to whoever sees the speaker;
  - world: to everyone in the game;
  - whispers: to the target, by pid or by name.
- **Other channels** are refused in M4, with an `ERR_CHAT_*` code that the plan picks from the client's own error table.
- **Limits.** The server enforces the client's limits again, plus its own rate (`ERR_CHAT_TOO_FAST`).

### 3.6 Admin commands

Accounts in `[access].admins` type commands in chat with `!`, since `/` lines never leave the client:
- `!tp <name>` takes the admin to a player, and `!tp <name> <name>` takes one player to another;
- `!kick <name>`;
- `!announce <text>` shows the marquee (channel 66);
- `!invite [label]` makes a new invite link and whispers it back.

A non-admin's `!` line is ordinary chat. The commands come from one module in fenji-server that the host console reuses (M4 spec §4.3, amended for the `!`).

### 3.7 Time syncs

From 772 on, each type-2 time sync gets an answer: the client's `u32` echoed back, and the server's time in milliseconds (#91). `transport.md` 10.6 gets its correction from the script first.

### 3.8 Safety

- **The never-send list.** 68 and 714 join fenji-proto's never-send list, and the world's other messages go through it like the lobby's.
- **The docs.** README.md, PRIVACY.md and SECURITY.md say plainly that joining a server means trusting its host: the client obeys server messages that can write files and run scripts on the player's machine.
- **The launcher's spec** decides whether the launcher should also guard players by filtering such messages.

### 3.9 Architecture

As the parent spec's §6.4 sets out:
- a headless bevy_ecs world at 20 Hz (`server.tick_hz`);
- fed over channels by the gate's per-connection tasks, with an outbound channel per connection;
- the store persists characters off the tick.

The world takes over a connection once the client sends 10. M4 adds the components it needs (account, connection, character, position, movement, interest) and these systems: inbound, movement, interest, chat and commands, and outbound.

## 4. How the work runs

### 4.1 Vertical slices

Each slice is done in four steps:
- read in the exe;
- written up in `docs/protocol/` with REA evidence IDs;
- built test-first, with `fenji-proto` round trips;
- run against the real client in the sandbox.

The slices, in order:
1. **Entering:** 772, type 3, the player's own actor and its speed, type 8, and time syncs. A character stands in the scene.
2. **Seeing each other:** types 6, 5, 7, 10 and 9, with two clients.
3. **Movement:** the move messages in both directions, and the speed check.
4. **Chat and admin commands.**

The native reading follows CLAUDE.md's method: the shared Ghidra project for exploring, REA for the evidence.

### 4.2 In parallel, in the cloud

The client-independent parts go to cloud agents, on Opus 5.5 at max effort. They get the public repository only, never anything from `re/` or the client. Their parts:
- the ECS world and its tick;
- the interest grid and the speed check;
- chat routing and limits;
- the command module;
- the store's migrations;
- the bot harness.

They build against interfaces the plan fixes, and the local slices plug the protocol in.

### 4.3 Two real clients

The runners gain a second client in their one namespace: two accounts and two copies of the client, against the one server. No path opens between namespaces, and the watchdog covers both clients. The first check is whether the two can share a Wine prefix.

### 4.4 The protocol bot

A `fenji-proto` client that signs in, enters, moves and chats. It runs in CI against a test server, and 20 of them measure the tick against the parent spec's §6.7 targets.

## 5. Risks and gates

- **The property blob's format** is the largest unknown. Reading it in the exe is the first slice's core.
- **The default scene** may need the original server's scene scripts to be playable. If so, the runs pick another one.
- **The gates.** If any step would need patching the client or loading its driver, work stops for the owner's decision (ground rule 6).

## 6. Open questions

The exe reading and the runs answer these:
1. The property blob's byte format, and which properties a player needs for a correct look and HUD.
2. The bodies of types 3 (21 bytes) and 5 (18 bytes), and of the move messages in both directions. How often moves are sent.
3. Whether the speed must be set by type 21 or 23, and the units of position and yaw.
4. Whether the server may push actors without waiting for the client's pull, and whether other players animate from relayed moves alone.
5. Whether the lobby's pid serves as the world id, and whether ids above 2^32 survive.
6. The default meeting scene.
7. Whether two clients can share a Wine prefix.
8. What filter value 32, which the client sends for visible actors, asks for.
