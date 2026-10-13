# Fenji — Design Spec

- **Date:** 2026-10-11
- **M0 updates:** 2026-10-12, from [`docs/re/spike-0.md`](../../re/spike-0.md): notes in §3, §4, §5.2–§5.4 and §6.2, and the "Answer (M0)" column in §12.
- **M1 updates:** 2026-10-12, from [`docs/re/m1-summary.md`](../../re/m1-summary.md): a note in §10 and the "Answer (M1)" column in §12.
- **Status:** Approved in brainstorming (sections 1–3); awaiting written-spec review
- **Scope:** Whole-project architecture and milestone map. Each milestone (M0–M5) gets its own implementation plan in `docs/superpowers/plans/`.
- **Lineage:** Fenji replaces Caphras, a Black Desert Online emulator halted on 2026-10-11 in favour of SoLO, whose client can be read; BDO's is packed. Fenji keeps Caphras's tooling, process and server architecture. Everything specific to the game is new (§13).

## 1. Purpose

Fenji is a clean-room private server emulator for **Swords of Legends Online (SoLO)**, written in Rust. SoLO is the Western build of 古剑奇谭网络版 (Gujian Online, developed by Aurogon and published in China by Wangyuan Shengtang). Gameforge operated it from 2021 until its servers shut on 2023-06-30 (Steam app 1418100, delisted on 2023-06-01). Anyone can host a Fenji server for a small group of friends they invite, with rates the server's admin tunes; it isn't tied to any one host.

Success for v1: invited friends sign in through the Fenji launcher, each with their own legitimately obtained copy of the client. They create characters, enter the starting zone, see each other, fight monsters, gain XP, loot items and keep their progress. The admin can change the server's rates live.

## 2. Ground rules (non-negotiable)

1. **Legitimate copies only.** Every player and the server host needs their own legitimately obtained copy of the SoLO client. That means Steam (app 1418100 already in their library, since it was delisted) or Gameforge's own launcher while SoLO was distributed there. Fenji never distributes the client, never points to unofficial copies, and never supports cracked or modified clients. There is no automated ownership check: the host vets everyone they invite (rule 5).
2. **Clean room.** Everything is derived from our own analysis of our own licensed client, and from watching it run against our own server. Leaked server files, server binaries, server scripts or server source for SoLO / 古剑奇谭网络版 are never used, referenced or accepted in contributions, and neither is anything derived from them. That covers:
   - write-ups based on such leaks, for example the GuJian Online "server RE report" circulated on RaGEZONE, recorded as rejected in `docs/re/sources.md`;
   - anything from the unofficial servers that appeared after the shutdown.

   Community material is used only if it is clearly licensed and has clean provenance, only as hints or as external tools, and only once listed in `docs/re/sources.md`.
3. **No redistribution of game content.** None of the following goes in the repo or in releases:
   - game binaries, data archives and extracted assets;
   - extracted or decompiled Lua;
   - middleware libraries (Oodle and others);
   - captures and REA evidence bundles.

   The server and tools read data from the operator's own installation at runtime. CI enforces this (§9).
4. **Never touch official services with modified or instrumented clients.** The original game is still operated in China, and Gameforge, Steam and Epic services are live. Instrumented runs (Wine API logging, debuggers) happen only inside `scripts/spike/isolated-run.sh`, connected to our own stub or server. (Owner's decision, 2026-10-13, #114: guarded bare-metal play tests on the Linux game box are the one exception, under the guards in CLAUDE.md.)
5. **Private and non-commercial.** The server is invite-only, unlisted, and never monetized.
6. **Decision gates stop the work.** Work stops and the owner decides explicitly when a step would:
   - patch the client, on disk or in memory;
   - circumvent any protection;
   - load the kernel driver embedded in the client (§5.4);
   - use proprietary middleware outside its licence, for example running Oodle's DLL on our server.

   These are never default plan steps.
7. **Credentials.** Fenji never handles Steam or Gameforge credentials; players type those only into DepotDownloader or the services' own pages. Fenji's own account passwords are stored only as argon2id hashes.

Legal context the owner has acknowledged:
- SoLO's EULA and terms likely prohibit reverse engineering and emulation.
- Copyright and anti-circumvention law apply.
- The rights holder still operates the game in China, and publishers do pursue private servers.
- Oodle is Epic Games' proprietary middleware.
- "Preservation" is not a legal safe harbour for an online server.

The rules keep exposure as low as possible; they don't eliminate it.

## 3. Environment

| Machine | Role |
|---|---|
| macOS (arm64) dev workstation | Rust development; static analysis (Ghidra, REA) of a copy of the pinned client kept outside the repo |
| Fedora Linux game box | Runs the pinned client through GE-Proton inside the sandbox, against the stub or dev server (and, since 2026-10-13, guarded bare-metal play tests against a local Fenji server; CLAUDE.md); Rust; analysis |
| VPS (Debian/Ubuntu) | Hosts `fenji-server`, Caddy, SQLite and the admin's own copy of the client data |

**M0 update.** Sandboxed runs start GE-Proton's own `proton` script directly, because umu's runtime container fails inside the sandbox (seen without `--cpu-topology`; untested with it). Wine there needs `--cpu-topology` for its processor information ([spike-0 §8](../../re/spike-0.md#8-runtime-observation-isolated-stub-only)).

Players: Windows and Linux (Proton). macOS players are out of scope.

**Pinned client build.** Fenji targets exactly one build: Steam app 1418100, depot 1418101, manifest 5002037851744157048, build ID 10015727. Its main executable is `bin64/gujianol.exe`, FileVersion 2.1.19.42851. The build has been frozen since the shutdown. M0 records the executable's size and SHA-256 and the file list in `docs/re/pinned-build.md`. The pinned copy lives in `$FENJI_CLIENT_PATH` (default `~/fenji-client`). It is either fetched with DepotDownloader under the person's own Steam login or copied from their own existing install.

**M0 update.** [`pinned-build.md`](../../re/pinned-build.md) records the file count and total bytes; for the file list it refers to Steam's cached depot manifest, which the pinned copy matches file for file.

## 4. Protocol discovery

A static triage on 2026-10-11 found the following (M0 wrote it up with evidence in `docs/re/spike-0.md`, which corrects two of its figures; the figures below are M0's):
- **Unprotected executable.** `gujianol.exe` is an ordinary MSVC 2013 x64 build, with a full exception table (75,016 entries; Ghidra finds 51,808 functions) and named imports.
- **Readable protocol.** The network layer is readable, and so are the protocol opcode enums (`STC_PROTOCOL_*`, `CTS_PROTOCOL_*`) and about 2,500 synced-property names (`prop_*`); the schema's exact size is not yet known.
- **Logic in Lua.** Much of the game logic is Lua 5.3 bytecode embedded in the executable. The Lua is modified: a custom header and 7 extra opcodes.
- **Data.** The sampled archive entries are Oodle-compressed or stored, with no sign of encryption.
- **Anti-cheat.** No third-party anti-cheat ships. An in-house kernel driver ("AGT") is embedded as a resource, with no visible load path; no M0 run extracted or loaded it (§5.4).
- **Boots without servers.** With the servers gone, the client still reaches its login scene.

**M0 update.** The last point holds twice: in the owner's launch through Steam ([spike-0 §7](../../re/spike-0.md#7-the-owners-launch-2026-10-11)), and with the unmodified client started directly in the sandbox ([§8](../../re/spike-0.md#8-runtime-observation-isolated-stub-only)), with `--gf-sdk=mocked` (dev-only, #11), `--fs=data:_index:_index` and `--cpu-topology`. Without a usable platform SDK the client exits about 50 ms in, inside `gameforge_api64.dll`'s initialisation. With all three, it reaches the login scene and stops at "FAILED TO FETCH GAMEFORGE SERVER LIST!", without a name lookup through Winsock or any connection.

**A: static analysis (the backbone).**

| Stage | Goal |
|---|---|
| 1. Network layer | Framing, header scramble, Oodle Network setup, handshake variant selection, send/receive state machine |
| 2. Dispatch | Opcode → handler mapping in both directions, native and Lua |
| 3. Payloads | The payload encoding, unknown so far ([spike-0](../../re/spike-0.md) 4.8–4.9), and the schema per opcode; the synced-property schema |
| 4. Scripts | Extract, disassemble and decompile the embedded Lua; read the client's own protocol and gameplay scripts |
| 5. Data | Archive and index format; table schemas |

The shared Ghidra GUI project (`ghidra` MCP) is the primary analysis surface. REA's full auto-analysis of an executable with about 52,000 functions is too slow to be the main surface. REA records Evidence IDs for targeted functions.

**M2 update** ([`docs/re/README.md`](../../re/README.md#the-norm), #18). Measured on the pinned exe: REA analyses all of it once per session, in about four minutes, then answers each query in seconds, and it doesn't see the GUI project's names. So the GUI project stays the exploration surface and REA is the evidence ledger: a new finding cites an REA Evidence ID next to the pinned-exe address. Addresses alone stay acceptable for findings written before the norm, for what only the GUI project holds, and where REA has no function.

**B: the real client against our stub, in the sandbox (validation).** No captures of the official servers exist, so the client is the only ground truth. The stub (`tools/stub`) logs every byte the real client sends and answers with our current best guess. The client's reaction refines the guess: its next packet, a stall, an error, Wine logs or its own `gujianol.log`. Every run happens inside `scripts/spike/isolated-run.sh`: loopback-only network, a nested display, and non-root.

**C: community material.** Only clearly licensed tools and notes with clean provenance, checked before use. Examples are Lua 5.3 decompilers, Oodle-compatible decoders and archive notes.
- Permissively licensed code may be linked (cargo-deny enforces the list).
- Anything else is used only as an external tool whose output stays in `re/`.

Every source is credited in `docs/re/sources.md`.

**Evidence handling.** Raw sessions, analysis databases, evidence bundles, decompiled output and extracted scripts live in the git-ignored `re/` directory. The repo holds only our own prose findings (`docs/protocol/`, `docs/re/`) and Rust definitions. Each cites REA Evidence IDs, or addresses in the pinned executable where REA can't reach in reasonable time.

## 5. Accounts, launcher and client redirect

### 5.1 Accounts (`fenji-auth` behind an `axum` HTTP API, Caddy for TLS)

1. The admin issues one-time invite codes (`/invite` in game or the server CLI), each optionally labelled with who it's for.
2. A player redeems a code in the launcher and chooses a username and password. The server stores an argon2id hash. One account per invite code.
3. Sign-in exchanges username and password for a session token. Attempts are rate-limited, with a temporary lockout after repeated failures.
4. On Play, the launcher exchanges the session token for a single-use **game token** (60 s TTL, bound to the account), which the login listener validates. How the token reaches the client: §5.3.

There is no Steam sign-in and no ownership check (rule 1).

**M3 update** ([`m3-summary.md`](../../re/m3-summary.md)). Built: one-time invite codes (stored as SHA-256 hashes, with labels and expiry), registration with an argon2id hash, sign-in with a limiter (a username only slows down; a pair of username and network, or a network, locks), session tokens, and single-use game tokens (`fenji1-` and 64 hex digits, stored as SHA-256, 60 s by default, configurable from 10 s to an hour, bound to one account). The account API speaks plain HTTP, and `fenji-launch` refuses `http://` except to this machine; hosting elsewhere needs M4's TLS proxy (Caddy, §8). `/invite` in game waits for chat (M4).

### 5.2 Launcher (Linux + Windows)

The launcher replaces Gameforge's `SOLO_Launcher.exe`, whose server-list service is dead. It is a **Tauri 2** desktop app (`apps/launcher`) with a small Vite + TypeScript frontend. All logic lives in the `fenji-launcher` library crate, so it's testable without a UI; the Tauri shell only wires commands and events to the frontend. UI work follows the `impeccable` skill.

- **Client source:** either an existing install the player points to, used in place if it verifies, or a DepotDownloader download of the pinned build under the player's own Steam login. With DepotDownloader the player authenticates directly; Fenji never sees credentials.
- **Verification:** the files the client loads are checked against the pinned build's hash manifest; only hashes are shipped. A mismatch blocks Play and says which files differ.
- **Launch:** `gujianol.exe` runs directly: on Linux through `umu-launcher`/Proton in a dedicated Wine prefix, on Windows natively.

**M0 update** ([spike-0 §5](../../re/spike-0.md#5-startup-chain-server-address-and-ticket), [§8](../../re/spike-0.md#8-runtime-observation-isolated-stub-only)). By static reading, `SOLO_Launcher.exe` picks a region and starts the game, and the game fetches the Gameforge server list itself; at run time it is the game that logs the failure. Started directly, `gujianol.exe` needs `--fs=data:_index:_index` to mount its archives, and a usable platform SDK, which `--gf-sdk=` chooses (without the flag the library defaults to `gf`, which needs the Gameforge Client's DLL); both are launcher design inputs, and which SDK players get is M3's decision (§5.3). What the original launcher passed to the game is unknown.

**M3 update** ([`m3-summary.md`](../../re/m3-summary.md)). `fenji-launcher` and its command line, `fenji-launch`, are built:
- **redeem** creates the account;
- **verify** checks a folder against the pinned build's manifest (210 files, quick or full);
- **play** starts the client on Linux through `umu-run`, in the launcher's own prefix, after `umu-run createprefix`. It starts only the pinned build, and refuses every other system: native Windows waits on the AGT reading (gate #114, plan Amendment B).

By the owner's decision of 2026-10-13, the Tauri app moved out of M3 to #146, with its screen designs approved by the owner and DepotDownloader download and repair in its brief.

### 5.3 Client redirect and ticket (preference order; M0 and M3 decide)

1. **Inputs the client already accepts**, without modifying any file: its command-line flags (`--gf-appid`, `--steam-appid`, `--gf-region`), `solo_steam.cfg`, `gfclient.ini` and `loader.ini`.
2. **Standing in for the dead Gameforge side.** The launcher provides what Gameforge used to provide: the region server list, and the ticket over the Gameforge client's named pipe. That talks to the client through its own interfaces; it is not patching.
3. **A hostname redirect**, active only while playing.
4. **Patching the client:** an owner decision gate (rule 6).

The client obtains an auth ticket through `gameforge_api64.dll`, which has Steam, Gameforge and Epic paths. M3 decides what the client sends our login listener and how we bind it to the Fenji game token. If the client turns out to need a running Steam client, M3 documents that requirement and how it affects players without a Steam licence.

**M0 update** ([spike-0 §5](../../re/spike-0.md#5-startup-chain-server-address-and-ticket), [§8](../../re/spike-0.md#8-runtime-observation-isolated-stub-only)):
- Option 1: the inputs that matter so far are flags, read only in `--name=value` form: `--gf-sdk=` (`gf`, `steam`, `mocked` or `mailru`), each SDK with its own app-id flag and, for `gf` and `mocked` only, a region flag, and the game's own `--fs=`. `mocked` passed without its app-id flag, and its region flag changed nothing visible. Up to its login scene the game didn't look for `solo_steam.cfg` or read `loader.ini` through the INI API, and the library read only a log level and a password from `gfclient.ini`. The client looks for a `servername` file at its install root and has `--server-name`, `--server_locale` and `--net` flags: candidates for the server address.
- Option 2: on the `gf` path the library doesn't talk to a pipe itself. It loads an in-process DLL, `gameforge_client_api64_v2.dll`, from the Gameforge Client's install (the `InstallLocation` of its Uninstall key); any pipe would sit behind that DLL (spike-0 8.6). `GC_PIPE_NAME` belongs to the library's Mail.ru module. A stand-in for that DLL would run our code inside the client process (#11).
- Option 3: the client looked up no host name through Winsock, so there is nothing to redirect yet.
- The library's `mocked` SDK, a built-in test backend, passes platform init without Steam or the Gameforge Client. The owner allowed it for sandboxed dev runs on their own copy only, never for players (#11).
- Ticket: the Steam SDK loads Steam's `steamclient64.dll`, so it needs a Steam client on the player's machine, most likely running; the `gf` SDK needs the Gameforge Client's DLL. The player path is M3's decision.

**M3 update** ([`login.md`](../../protocol/login.md) §7, [`lobby.md`](../../protocol/packets/lobby.md) §12). Option 1 won: the launcher path. The launcher puts `gj_uinfo`, `gj_gates` and `gj_ticket` in the unmodified client's environment. The client then skips its SDKs and sends our single-use game token as its auth payload's first value. No `--gf-sdk` flag and no Steam client are needed (run m3-l6). The owner chose this path for players (gate #114, option a, 2026-10-13). The token can't stop an attacker on the network path, since the transport is readable and unauthenticated, so no password crosses it, and PRIVACY.md says so.

### 5.4 Kernel driver gate

The executable embeds a kernel driver ("AGT") with no visible load path in this build. Before the first client run, M0 confirms it is never extracted or loaded. If any step would load it, work stops (rule 6).

**M0 update** ([spike-0 §3](../../re/spike-0.md#3-anti-cheat-and-the-embedded-agt-kernel-driver), [§8](../../re/spike-0.md#8-runtime-observation-isolated-stub-only)). The sandbox alone doesn't prevent a load: Wine hosts kernel drivers in user mode, in `winedevice.exe`, as it does for its own drivers in every run. M0's answer: in runs 1–9, all under Wine/Proton, the client neither extracted nor loaded AGT, up to its login scene. Every run was checked afterwards, and a driver watchdog also watched runs 5–9. On native Windows, which no run covered, the answer rests on the static reading (spike-0 3.11). Code paths after login and in game are not yet covered.

## 6. Architecture

### 6.1 Monorepo layout

```
fenji/
├── Cargo.toml                 # workspace
├── rust-toolchain.toml        # pinned Rust
├── crates/
│   ├── fenji-proto/           # framing, header scramble, Oodle Network codec, handshake, payloads, opcodes (sans-I/O)
│   ├── fenji-data/            # archive + index reader, Oodle LZ decompression, table parsers → typed GameDb
│   ├── fenji-lua/             # the client's modified Lua 5.3: chunk extraction, disassembler, decompiler; later a VM (§6.4)
│   ├── fenji-auth/            # invite codes, accounts (argon2id), sessions, game tokens
│   ├── fenji-store/           # SQLite persistence (sqlx)
│   ├── fenji-server/          # binary: login + world listeners, ECS world, admin, config
│   └── fenji-launcher/        # library: sign-in, client verification, client launch
├── apps/
│   └── launcher/              # Tauri 2 shell (src-tauri/) + Vite/TypeScript frontend
├── tools/
│   ├── stub/                  # stub server for the discovery loop (ported from Caphras)
│   ├── archive/               # CLI: list archives, dump tables
│   └── luatool/               # CLI: list, disassemble and decompile embedded Lua (output stays in re/)
├── content/                   # our own authored content overlays (TOML)
├── deploy/                    # systemd units, Caddyfile, backup timer
├── docs/                      # specs, plans, protocol findings, RE notes
├── scripts/                   # bootstrap.sh, versions.env, checks, spike/isolated-run.sh
└── re/                        # LOCAL ONLY, git-ignored: analysis sessions, evidence, extracted data
```

Crate dependency direction:
- `proto`, `lua`, `auth` and `store` depend on nothing internal; `data` may depend on `lua`.
- `server` depends on all five; `launcher` on `proto` and `auth`; `apps/launcher` on `launcher`.
- Tools depend on `proto`, `data` and `lua`.

Key dependency versions at design time are the same as Caphras's: `tokio` 1.53, `binrw` 0.15, `bevy_ecs`/`bevy_app`/`bevy_remote` 0.20 with `bevy_brp_extras` 0.23, `axum` 0.8, `sqlx` 0.9 (SQLite), `reqwest` 0.13, `serde` 1, `toml` 1.1, `notify` 8.2, `arc-swap` 1.9, `tracing` 0.1, `clap` 4.6, `thiserror` 2 and `tauri` 2.12. New for Fenji: RustCrypto's `argon2` for password hashes. Exact pins go in `Cargo.toml`/`Cargo.lock` when each crate is created.

### 6.2 `fenji-proto` (pure, no network I/O)

- `frame`: stream → frames, each with a little-endian `u32` length prefix (from the triage; M2 confirms it).
- `scramble`: the rolling XOR over frame headers, with a separate key per direction.
- `codec`: a `StreamCodec` trait. The Oodle Network implementation is clean-room, or handled under a decision gate (rule 6). `NullCodec` is for tests.
- `handshake`: a sans-I/O state machine, including the gate handshake that selects one of the codec variants together with the shared dictionary.
- `payload`: the `pickle` and msgpack-style value codecs, plus typed packets per opcode, each doc-commented with its evidence.
  - `Opcode` is a `#[non_exhaustive]` enum.
  - Unknown opcodes decode to `Packet::Unknown { opcode, bytes }`, never a panic.

**M0 update** ([spike-0 §4](../../re/spike-0.md#4-network-layer)). The triage's `u32` length prefix and `pickle` body belong to the client's built-in remote debugger, not to the game transport. By static reading, each game message has a 4-byte header with a 24-bit length and the message type, inside a stream scrambled with a 16-bit key per direction. The client only decodes Oodle Network data, so our server has to send data that its decoder accepts. Which serialiser the payloads use is unknown. M2 confirms all of this.

**M2 update** ([`m2-summary.md`](../../re/m2-summary.md), [`transport.md`](../../protocol/transport.md)). Confirmed against the real client, with these corrections to the bullets above:
- **Frames:** a frame is a `u16` length XOR'd with the direction's key, then the body. Length 0 is a time sync. After each frame the key changes: server to client to `k + 0xD736`; client to server to `(k + 0x28CA) XOR k` after a data frame and to `(k + 0xB4FB) XOR k` after a time sync.
- **Scramble and encryption:** the rolling XOR covers only each frame's `u16` length and a time sync's four words, with one key per direction (`scramble`). Each data frame's body is encrypted with the stream variant the hello picked for its direction (Fenji uses variant 3 both ways: AES-128 ECB plus a tail XOR). The keys come from the hello, which travels in the clear, so this hides nothing from anyone who sees the stream: tickets must be single-use and short-lived, and passwords never cross it ([`transport.md`](../../protocol/transport.md) 3.9).
- **The inner header:** 4 bytes, a 24-bit length and the type, scrambled.
- **Oodle Network:** server frames are an Oodle Network packet behind a raw-length prefix. A stored packet (raw length = packet length) passes the client's decoder, so the server needs no encoder and no gate.
- **Payloads:** the client's own pickle, LZ4-compressed from 128 bytes up. There is no msgpack.
- **Unknown opcodes:** they decode to `Unknown`, as planned.

### 6.3 `fenji-data`, `fenji-lua` and content layering

- **Client-derived (runtime only, never committed):** tables (items, skills, quests, monsters, maps), the XP curve, the embedded Lua scripts, and terrain later.
- **Authored overlays (`content/*.toml`, committed):** values that only ever existed server-side and that the client doesn't carry, such as drop tables, spawn data (if absent from the client), missing stats and formula parameters.
- `GameDb` is the client-derived data merged with the overlays. Overlays win on conflict, and loading fails loudly on schema errors.
- `fenji-lua` parses the modified bytecode format (header and extra opcodes) into a typed representation. `luatool` writes disassembly and decompiled source to `re/` only.

### 6.4 `fenji-server`

- A `tokio` runtime with one task per connection. Each task decodes packets and forwards `Inbound { conn, packet }` over a channel to the ECS world. Per-connection outbound channels carry encoded packets back.
- Login and world listeners run in one process, and support both one- and two-connection client flows.
- A headless `bevy_ecs` app with a fixed tick (default 20 Hz, `server.tick_hz`). One world, one channel.
- The client's synced-property model (the `prop_*` schema, `STC_PROTOCOL_SYNC_ENTER/LEAVE/UPDATE`) maps onto ECS components; M4 defines the mapping.

Components: `Account`, `Connection`, `Character`, `Position`, `Movement`, `Vitals`, `Level`, `Experience`, `Inventory`, `Cooldowns`, `Monster { template }`, `SpawnPoint`, `AggroTable`, `AiState`, `GroundLoot { owner, expires }`.

Systems per tick, in order:

1. **Inbound:** apply queued packets.
2. **Movement:** accept client positions that pass a speed check (`max_speed × dt × tolerance`); otherwise rubber-band. Lenient by design.
3. **Interest management:** a 50 m grid. A player sees their own cell and the 8 neighbouring cells; entering or leaving a cell sends spawn or despawn packets.
4. **Monster AI:** Idle → Aggro → Chase → Attack → Leash to spawn → Dead → Respawn after `base × rates.respawn`.
5. **Combat:** validate skill use (range, cooldown, resource cost), compute results, broadcast them.
6. **Progression:** XP is the overlay value × `rates.xp`, with level-ups from the client's XP curve. Drops are rolled from `content/drops/*.toml` × `rates.drop`. Ground loot belongs to the killer.
7. **Outbound:** send to interested connections, and mark dirty characters.

**Hybrid rule.** Rust owns networking, the ECS world, movement, interest management, persistence and accounts. At the start of M5, each gameplay system is decided on its own: skills, buffs, effects, damage formulas, AI. Either the system runs through the client's own scripts, or Fenji implements it in Rust. Scripts are loaded at runtime from the operator's install by a `fenji-lua` VM and never redistributed. The M5 plan records each decision with the evidence from M1.

### 6.5 `fenji-store`

SQLite (WAL) via `sqlx`. Tables: `accounts` (username, argon2id hash, created, last sign-in), `invites`, `characters`, `character_stats`, `inventory_items`, `character_skills`.
- Characters are saved on logout, on level-up, every 60 s if dirty, and all of them on SIGTERM.
- Each character save is one transaction.

### 6.6 Admin

Admins are account names in `[access].admins`. Chat commands (typed with `!` in place of `/` from M4 on, since the client keeps `/` lines to itself; [M4 world spec](2026-10-13-m4-world-design.md) §2.4): `/reload`, `/rate <name> <value>`, `/give <item> [n]`, `/spawn <monster>`, `/tp`, `/level`, `/kick`, `/invite`, `/announce`. BRP (Bevy Remote Protocol, via `bevy_brp_extras`) binds to localhost only, so it is reachable over an SSH tunnel; the `brp` MCP uses it for live world inspection.

### 6.7 Capacity and performance targets

Design capacity is **20 concurrent players** (`server.max_players`, default 20). A sign-in beyond the cap is refused with a "server full" message. Everything is sized for that, not for an MMO:

- One process, one world, one SQLite file. No sharding, no separate database server, no horizontal scaling.
- At 20 Hz the tick budget is 50 ms. The target is p99 tick work ≤ 10 ms with 20 players in the starting zone and the monsters around them. A tick over budget is logged with the slowest systems.
- Character saves run on the tokio runtime, never inside a tick. The target is a full save of all 20 characters (SIGTERM) in ≤ 2 s.

These are targets to measure against from M4, not promises. An admin adjusts them for their own machine and player count.

## 7. Configuration (`fenji.toml`)

```toml
[server]
public_host = "play.example.net"
login_bind  = "0.0.0.0:<port>"   # ports chosen in M2–M3; the client gets its address from us
world_bind  = "0.0.0.0:<port>"
tick_hz     = 20
max_players = 20

[data]
client_path  = "/srv/fenji/solo-client"
pinned_build = "10015727"
content_path = "/srv/fenji/content"

[access]
invite_only = true
admins      = []

[rates]
xp      = 1.0
drop    = 1.0
money   = 1.0
respawn = 1.0

[combat]
crit_multiplier = 1.5
```

Hot reload (a file watcher or `/reload`) parses and validates the whole file, then swaps it in atomically (`arc-swap`).
- An invalid file leaves the old config in place; the error is logged and whispered to online admins.
- `[rates]`, `[combat]` and `[access]` reload live. Changes to `[server]` and `[data]` are reported as "restart required".

M5 may add or rename rates once SoLO's progression systems are mapped.

## 8. Deployment (VPS)

Anyone can host Fenji. The reference setup below is a small Linux VPS; a host can adapt it to their own machine.

- A release binary run by systemd: `StateDirectory=fenji`, `ProtectSystem=strict`, `NoNewPrivileges=yes`, `PrivateTmp=yes`. Secrets (the token-signing key) come via `EnvironmentFile=/etc/fenji/secrets.env`. No Docker.
- Caddy 2.11 reverse-proxies the account API with automatic HTTPS.
- `ufw` allows 443 and the game ports only; BRP and admin tooling stay on localhost.
- The admin copies their own client data into `/srv/fenji/solo-client`, read-only to the service. The full install is about 100 GB of archives. M1 measures the subset the server actually needs, so the VPS may hold only that subset, copied from the admin's own install and never shared.
- A nightly systemd timer runs SQLite `.backup`, with 14-day retention.
- Sizing: at least 4 GB RAM; disk per M1's measurement.

## 9. Testing and CI

- **`fenji-proto`:** golden round-trip tests (bytes → struct → bytes). Codec tests use frames the real client sent to our stub inside the sandbox, the only ground truth available. Fixtures from those runs are minimal and scrubbed before commit.
- **`fenji-data`, `fenji-lua`:** integration tests gated on `FENJI_CLIENT_PATH`, skipped in CI (there are no game files there).
- **`fenji-server`:** deterministic ECS tests with fake connections (e.g. a kill at `rates.xp = 3` gives 3× XP).
- **Bot client:** built on `fenji-proto` (login → enter world → move → attack → loot), it runs in CI. It validates the server against *our* understanding of the protocol, so each milestone also requires a manual run with the real client in the sandbox.
- **CI (GitHub Actions):** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo deny check`, ShellCheck, the commit-subject and PR-title check, the sandbox self-test, and `scripts/check-forbidden-files.sh`. The last rejects:
  - PE/DLL/driver binaries, `data0NN`/`_index` archives, Lua bytecode and decompiled script dumps, and Oodle libraries;
  - pcaps, REA bundles and Ghidra/Hopper databases;
  - anything under `re/`.

## 10. Milestones

| # | Milestone | Done when |
|---|---|---|
| M0 | Dev environment + spike | Tooling ported and CI green. Pinned build recorded. Triage written up in `docs/re/spike-0.md` with evidence. The sandbox runs the client non-root on a nested display. One real-client run against the stub answers: does it start without `SOLO_Launcher`, what does it send first, and does the AGT driver stay unloaded? Gates resolved |
| M1 | Data layer | `archive` lists archives and dumps items, skills, monsters, XP curve and maps. `luatool` disassembles the embedded Lua and decompiles enough to read the protocol and combat scripts. How much server-side logic the scripts carry is measured. The Oodle-for-archives path is decided |
| M2 | Transport + handshake | The stub completes the real client's gate handshake and decodes its first packets with `fenji-proto`. The Oodle Network path is decided |
| M3 | Login + lobby | Invite accounts, launcher (verify, launch), ticket handoff, and character list/create/delete in the real client |
| M4 | Social sandbox + VPS | Friends enter the starting zone, move, see each other and chat; admin commands; the reference deployment (§8), written up so anyone can host their own server |
| M5 | Grind loop (v1) | Monsters, AI, combat (hybrid decision per system), XP, loot, inventory, persistence, live-tunable rates |

**M1 update** ([`m1-summary.md`](../../re/m1-summary.md)). M1 delivered:
- `luatool`, which lists, disassembles and decompiles all 20,443 embedded scripts;
- `archive`, which reads the archives' structure without decompression;
- the server-logic census;
- the tracked client runner with its AGT watchdog;
- the real client's first session with fenji-stub.

By the owner's ruling at gate #39 (option d, 2026-10-12), the archives' compressed content leaves M1, and with it the item, skill, monster, XP-curve and map dumps. They move to M2 (#51). There, `ooz` as an external tool or the client's own Oodle DLL in a sandboxed helper gets chosen, together with the Oodle Network question.

**M2 update** ([`m2-summary.md`](../../re/m2-summary.md)). M2 delivered:
- `fenji-proto`, the client's transport as sans-I/O code;
- `fenji-stub --speak`, with which the real client reached `WORK` and sent its first script message, decoded, on both login paths;
- the Oodle Network path, decided without an encoder (stored packets);
- the archives: the client's own Oodle DLL in a sandboxed helper (#51, route c, dev only), `_index`'s paths, and the design tables, which ship as one pickle cache; `archive dump` writes items, skills, monsters, the XP curve and maps under `re/`.

M0–M2 can redirect or stop the project (the kernel driver, Oodle licensing, the ticket handoff). That is intended: we learn it before building the server.

Each milestone release to `main` bumps the minor version (M0 → `0.1.0` … M4 → `0.5.0`); M5 releases `1.0.0`.

**M3 update** ([`m3-summary.md`](../../re/m3-summary.md)). M3 delivered:
- invite accounts and the account API;
- the gate and the character lobby, whose list, create and delete survive a restart;
- the ticket handoff on the launcher path;
- `fenji-launch`, which redeems, verifies and plays on Linux;
- the real, unmodified client through all of it, in the sandbox (runs m3-l1 to m3-l6) and on bare metal on Linux (bm-4 to bm-6).

The launcher's window (#146) moves to M4 by the owner's decision, and a console for hosts (#147) is a new request for M4.

**M4's plan** ([M4 spec](2026-10-13-m4-social-sandbox-design.md), 2026-10-13). By the owner's decisions, M4 is three pieces, each with its own spec and plan, in order: the world (the bar above, made exact), hosting (the reference deployment and the host console) and the desktop launcher. v0.5.0 ships when all three are in.

## 11. Conventions

SemVer 2.0.0 (one workspace version) and Conventional Commits 1.0.0. `dev` is the default and integration branch; `main` holds tagged releases only, and work branches are cut from `dev`. `CHANGELOG.md` follows Keep a Changelog and is generated by `git-cliff`. Full rules: `docs/development.md`.

## 12. Open questions (answered by milestones, not by guessing)

| Question | Answered in | Answer (M0) | Answer (M1) | Answer (M2) | Answer (M3) |
|---|---|---|---|---|---|
| Does `gujianol.exe` start and reach its login scene without `SOLO_Launcher`, inside the sandbox? | M0 | **Yes**, with `--gf-sdk=mocked` (dev-only, #11), `--fs=data:_index:_index` and `--cpu-topology`. It stops at the failed server-list fetch ([spike-0 §8](../../re/spike-0.md#8-runtime-observation-isolated-stub-only), [Conclusions](../../re/spike-0.md#conclusions)) | — | — | **Yes, with no `--gf-sdk` flag at all**, on the launcher path (run m3-l6; [`lobby.md`](../../protocol/packets/lobby.md) §14) |
| Is the embedded AGT kernel driver ever extracted or loaded? | M0 | **No**, up to the login scene, in runs 1–9 under Wine/Proton; on native Windows it rests on the static reading (3.11); later code paths are not yet covered ([§3](../../re/spike-0.md#3-anti-cheat-and-the-embedded-agt-kernel-driver), [§8](../../re/spike-0.md#8-runtime-observation-isolated-stub-only), [Conclusions](../../re/spike-0.md#conclusions)) | **Still no sign** through the first connection (runs m1-c1 and m1-c2, [`login.md`](../../protocol/login.md) §6), now with every resource lookup logged, none of the driver's type `PE` | **Still no sign** through `WORK`: the watchdog was clean in every M2 run, m2-r1 to m2-r5 ([`transport.md`](../../protocol/transport.md) §9) | **No sign** through the lobby, in runs m3-l1 to m3-l6 under Proton in the sandbox; in two of them the watchdog's winedevice rule wasn't armed (#142, since fixed), and its other rules were ([`lobby.md`](../../protocol/packets/lobby.md) §14). On bare metal on Linux, no sign in bm-2 to bm-5. bm-1's exit 3 was GE-Proton's own Discord bridge service, a user-mode program, so it was proven a non-AGT false positive ([`m3-summary.md`](../../re/m3-summary.md)). Native Windows remains unverified |
| What does the client send first, and where does it get its server address? | M0 | **Unknown at run time**: the client connected nowhere. By static reading the server speaks first, with a 64-byte hello. Where the address comes from is open; a `servername` file and the `--server-name` and `--net` flags are among the candidates ([§4](../../re/spike-0.md#4-network-layer), [§8](../../re/spike-0.md#8-runtime-observation-isolated-stub-only), [Conclusions](../../re/spike-0.md#conclusions)) | **Answered** ([`login.md`](../../protocol/login.md) §3, §6). The address comes from the server list at the `serverlist_url` setting and the player's choice in the server-choice frame (observed). The client sends nothing first: connected to fenji-stub, it waited 85 s in silence, so the server speaks first (inferred) | **Run:** the server sends its 64-byte hello, then the client sends its auth message ([`transport.md`](../../protocol/transport.md) §2, §6, §9) | — |
| Archive and index format; Oodle LZ: clean-room decoder, licensed external tool, or gate? | M1 | Partly: 100 archive parts whose record tables are only partly understood; the sampled entries are Oodle-compressed or stored, with no sign of encryption ([§6](../../re/spike-0.md#6-data-archives-and-scripts)) | **Format answered; Oodle deferred to M2 by the owner's ruling (gate #39, #51)** ([`m1-archives.md`](../../re/m1-archives.md)). Parts chain checksummed index blocks of SHA-1-keyed records; 93.6% of files are Oodle and the rest stored; the 1,135 stored files checked are plain. `_index` is one Oodle blob of path lines | **Oodle answered** ([`m1-archives.md`](../../re/m1-archives.md) 3.4–3.5, [`m2-tables.md`](../../re/m2-tables.md)). The client's own DLL decodes the blocks inside the sandbox (dev only). `_index` names 272,734 paths, and the design tables are one pickle cache | — |
| How much server-side logic do the embedded scripts carry? | M1 | Partly: 18,587 LZ4-wrapped chunks of modified Lua 5.3 in the exe, and much of the combat logic looks like scripts that run on both sides and branch on `IsServer`/`IsClient`; the amount is M1's to measure ([§6](../../re/spike-0.md#6-data-archives-and-scripts)) | **Measured** ([`m1-script-census.md`](../../re/m1-script-census.md)): 17.6% of `data/scripts`' instructions run only on the server, mostly in combat, and two `server/` directories ship whole | — | — |
| Oodle Network: clean-room codec, an uncompressed path, or gate? | M2 | Partly: the client imports Oodle's decoder but not its encoder, so the server has to send data that the decoder accepts ([§4](../../re/spike-0.md#4-network-layer)) | — | **Answered: an uncompressed path** ([`transport.md`](../../protocol/transport.md) 5.8, §9). The decoder accepts stored packets, so the server needs no encoder | — |
| Handshake variant selection and per-connection keying | M2 | Partly, by static reading: after the server's hello the client picks one of 27 stream variants, and each direction keeps its own 16-bit scramble key; untested ([§4](../../re/spike-0.md#4-network-layer)) | — | **Answered and run** ([`transport.md`](../../protocol/transport.md) §2–4, §9). The hello's check and the variants (w0 and w1 mod 27, unsigned) are confirmed, as are the keys per direction | — |
| One or two connections (gate vs world)? Ports? | M2–M3 | Open: no M0 run connected anywhere, so no port is known ([§8](../../re/spike-0.md#8-runtime-observation-isolated-stub-only)) | **Partly:** the gate's address and port come from the server list, so they are ours to choose; the client opened one connection, to the gate ([`login.md`](../../protocol/login.md) §6) | **Partly:** the whole run to `WORK` used one connection, to the gate. Whether a world connection follows is M3's to see ([`transport.md`](../../protocol/transport.md) §9) | **One, through the lobby:** the gate (7800 by default, ours to choose) carries login and the whole lobby; entering the world is M4's question ([`lobby.md`](../../protocol/packets/lobby.md) §14) |
| Ticket handoff: what the client sends, and whether it needs a running Steam client | M3 | Partly: the Steam SDK needs Steam's `steamclient64.dll`, so a Steam client, most likely running; the `gf` SDK needs the Gameforge Client's DLL; `mocked` is dev-only (#11). Which message carries the ticket is unknown ([§5](../../re/spike-0.md#5-startup-chain-server-address-and-ticket), [§8](../../re/spike-0.md#8-runtime-observation-isolated-stub-only)) | — | **Partly:** the auth payload carries the ticket as its first value: `gj_ticket` on the launcher path, the SDK's ticket on the server-list path (with the mocked SDK, a hex JSON object). Whether a real SDK needs a running client is M3's ([`payloads.md`](../../protocol/payloads.md)) | **Answered:** on the launcher path the client sends our single-use game token as the auth payload's first value, with no SDK flag and no Steam client; the owner chose that path for players (gate #114, option a) ([`login.md`](../../protocol/login.md) §7) |
| Which gameplay systems run the client's scripts (hybrid rule) | M5 | Not covered by M0 | — | — | — |
| Does the client report hits, or expect server-side hit detection? | M5 | Not covered by M0 | — | — | — |

## 13. Lineage from Caphras

Carried over, with only names changed (caphras → fenji, BDO → SoLO):
- the dev environment and pinned tool versions;
- the Claude Code configuration (plugins, MCP servers);
- the git workflow, CI, and issue and PR templates;
- the stub;
- the sandbox, `isolated-run.sh`, as reviewed in Caphras PR #25 (head `1e23272`).

The sandbox follow-ups left open by Caphras's reviews become Fenji issues in M0, the planned `--no-root` mode first. Caphras's BDO-specific documents (spec, plans, spike notes, pinned build) are not carried over.
