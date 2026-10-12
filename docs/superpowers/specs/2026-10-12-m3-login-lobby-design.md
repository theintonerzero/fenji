# M3: Login and lobby — Design Spec

**Status:** approved under the owner's overnight pre-approval (2026-10-12), after a self-review and a fresh reviewer's review against the parent spec and the ground rules, whose findings are folded in; every choice is a ruling in the M3 ledger.
**Parent:** [Fenji design spec](2026-10-11-fenji-design.md) §5 (accounts, launcher, redirect and ticket), §6.1, §6.4–§6.7, §7, §9, §10 (M3) and §12 (the ticket question), with M2's findings in [`docs/re/m2-summary.md`](../../re/m2-summary.md), [`docs/protocol/transport.md`](../../protocol/transport.md) and [`docs/protocol/payloads.md`](../../protocol/payloads.md).

## 1. Purpose and priority

M3 turns the stub's handshake into a server people can sign in to. It ends when the real, unmodified client, started inside the sandbox, logs in to `fenji-server` with a ticket that the launcher's code fetched, lists its characters, creates one and deletes one, and when a character survives a restart of the server.

As in M2, the priority is **the fastest route to the next real-client milestone**: here, a character created in the real client and listed again by a restarted server. The launcher's desktop shell and the admin's conveniences run alongside and never block that route.

M3 creates `fenji-auth`, `fenji-store`, `fenji-server`, `fenji-launcher` and `apps/launcher` (parent spec §6.1), extends `fenji-proto` and the client runner, and never changes the client. **No build of M3 that a player would run can start the client:** whether players may use the launcher path is the owner's open decision (#54, D2).

### What we already know

From M2 (**Observed** in runs, see the parent spec's M2 updates):
- The client opens one TCP connection to the gate it is given, checks our hello and sends its auth message. On the launcher path (`gj_uinfo` in the environment, with `gj_gates` and `gj_ticket`), the auth payload's values are, in order:
  - first, `gj_ticket` exactly as given;
  - third and fourth, the account and the user id from `gj_uinfo`;
  - fifth, `gj_uinfo`'s first field ([`payloads.md`](../../protocol/payloads.md), the auth payload).
- On that path the auth goes out 9–10 s after the client starts (runs en-1, m2-r1, m2-r4b and m2-r6: each run's start against the stub's session timestamp), long before the title screen appears.
- `ok`, then `PROTOG2C_SET_NET_CONNECTED` (414) with the four billing fields, moves the client to `WORK`. It answers with script message 402 and waits on its title screen, "Press Any Key" (run m2-r4b's screenshots).
- The transport hides nothing and authenticates nobody: anyone who sees the stream can read the ticket, and anyone on the path can pose as either side ([`transport.md`](../../protocol/transport.md) 3.9).

From reading the client's own scripts while designing M3 (`re/m1/decompiled-all/`, read-only; **Inferred**, to be written up with full evidence by W1):
- **The scripts use literal message ids.** `net.luac` registers its handlers under numbers and sends numbers. The protocol enum (`enum_protocols.luac`) assigns its names from a global and doesn't give the numbers.
- **The lobby's messages,** all over `CTS_PROTOCOL_GATE_SCRIPT` and `STC_PROTOCOL_SCRIPT` on the one gate connection:

  | Client function | Sends | Server message the client handles | Handler's effect |
  |---|---|---|---|
  | `QueryActors()` (`net.luac`) | first `hotpatch_pull` (`hotpatch_system.luac`): 370 with the client's hotpatch revision, then on a shipping client 496; then 292, no arguments | 228 (last pid, the list of actors, a table with `billing_gap`) | stores the list, fires `EVENT_QUERY_ACTORS` |
  | `CreateActor(t)` | 304, one table | 628 (the new actor, a table with `pid`) | adds it to the list, fires `EVENT_CREATE_ACTOR_RESULT` |
  | `DeleteActor(pid)` | 166, the pid | 84 (pid, a time in ms, a level) | fires `EVENT_DELETE_ACTOR_RESULT` |
  | `SelectActor(pid)` | 10, the pid | (entering the world: M4) | |
  | the 414 handler | 402, no arguments | unknown | |
  | any refusal | | 270 (an error code, then format arguments) | prints it, fires `ERR.EVENT_SERVER_ERROR` |

- **The title screen.** In `WORK`, a key press calls `QueryActors` (`Login.luac`, `OnGlobalKeyUp`). When the list comes back, the login screen plays a transition. Then it opens character creation if the list is empty, or character select if it isn't.
- **Actors are tables of the client's synced properties.** The select screen reads these, among others (`ActorSelect`, the `locale/common` copy):
  - `pid`, `prop_base_name`, `prop_base_lv` and `prop_base_vocation`;
  - `prop_base_skeleton`, derived from vocation and `prop_base_gender` when missing;
  - `prop_sys_portrait`, `prop_sys_avatar_feature` and `prop_sys_avatar_feature_face`;
  - `prop_sys_equips`, the `prop_sys_show_*` flags and `prop_sys_pendant_custom_positions`.
- **The create request** (`CreatePanel.luac`, `locale/common` copy) is one table with these fields:
  - `prop_base_name`, `prop_base_vocation` and `prop_base_skeleton`;
  - `prop_sys_player_voice`, `prop_sys_avatar_feature` and `prop_sys_avatar_feature_face` (the face editor's data);
  - `prop_sys_player_tattoo_pos` and `prop_sys_player_birthday`, both 0;
  - `prop_base_equips_id` and `prop_sys_portrait`.

  It carries no `prop_sys_equips` and no `prop_sys_show_*`.
- **The create result's handler asserts a table,** so a refused creation must come back as message 270. Its code is the `ID` of a row in the client's own `errors` sheet, such as `ERR_ROLE_NAME_EXIST` (read with `archive dump --table errors`).
- **The delete result** shows a "deletion pending" dialog only when the actor's level is at least the reply's third value. Then it asks for the list again.
- **Close reasons the client knows.** `net.luac` maps reasons such as `error auth-failed`, `error duplicate-login`, `error server-not-running` and `error auth-client-token-error` to texts of its own. Any other reason shows as "Internal server error: …" (run m2-r3b).
- **Code the server must never send.** The client takes hotpatch files (243), "GM scripts" (216) and Lua bytecode (556) from the server, and replaces or runs code with them (`hotpatch_system.luac`, [`transport.md`](../../protocol/transport.md) 10.7).
- **The Western client is a "common locale" build.** `IsCommonLocale()` returns true and `GetServerLocale()` returns 3, and the module loader rewrites `data/…` paths to `data/locale/…`. So the `locale/common` copies of `ActorSelect` and `ActorCreate` are the ones that load; W1 confirms it.

## 2. Decisions

The owner pre-approved this design on condition that every choice is a ruling in the M3 ledger (`.superpowers/sdd/2026-10-12-m3-login-lobby/progress.md`), with its reason and its cost if wrong. The table states the decisions; the ledger has their costs.

| # | Question | Decision |
|---|---|---|
| D1 | Which login path M3 builds and tests on | **The launcher path** (`gj_uinfo`, `gj_gates`, `gj_ticket`), carrying a Fenji game token as `gj_ticket`. It is the only path that carries a ticket we issue, and M2 ran it. It is used in sandboxed developer runs only, as M2's P4 allowed. `fenji-launch` never emits a `--gf-sdk` flag. When a run needs `--gf-sdk=mocked` (gate #11), the runner's caller passes it as M2's runs did, and L6 runs without it. |
| D2 | Player use of that path (#54) | **Decided 2026-10-13 by the owner: option (a), the launcher path for players (#114; plan Amendment B). The rest of this row is the original text.** **Not decided here.** A `Decision gate` issue goes to the owner at once, with options and a recommendation (§7). Until it is decided, no player-facing build can start the client: the `Uinfo` login path compiles only under the non-default cargo feature `dev-login-path`, which only the runner's developer build enables. `apps/launcher` never enables it; its Play button stays disabled and says why. `fenji-launch play` refuses without the feature, and M3 has no `play --exec` at all. |
| D3 | The ticket and its threat model | A **game token**: `fenji1-`, then 64 hex characters from 32 bytes of the OS's random source. The prefix keeps it from ever matching the client's `^%x+$` hex-decoding branch in `net_manager_login`, which its constant guard makes unreachable in this build anyway. The server keeps only its SHA-256. The token is single-use (consumed atomically when the auth message arrives), bound to one account, and valid for 60 s (the parent spec's figure; the client sends it about 10 s after starting). The auth payload's account (value 3) must be that account's username. Fenji's own code (server, stub, runner, launcher) never logs or prints the ticket field, whatever it holds, or a password or session token. **Threat model:** single use, the short life and the account binding stop later replay, tickets leaking from logs and observers off the path. They can't stop an attacker on the path, who can pose as the server, take the token and use it first, or take over the session after auth; only a change to the client could, and that is gated (rule 6). So the game connection carries no password, and the README and PRIVACY.md tell hosts and players that it is readable and unauthenticated; a VPN is the mitigation. |
| D4 | One connection or two | **One**, the gate, for everything M3 does. All lobby messages are gateway script messages. A second connection, if entering the world needs one, is M4's. |
| D5 | The server's shape in M3 | One `tokio` process with the gate listener, the account API (`axum`) and admin subcommands. **No ECS world yet:** the lobby is request and reply against the store, handled in each connection's task. The world, its tick and BRP arrive in M4. |
| D6 | Where the auth logic lives | `fenji-auth` holds pure functions and types: password hashing (argon2id), token and invite-code generation and hashing, the username and password rules, and the sign-in limiter. `fenji-server` runs the flows against `fenji-store`. Neither library depends on anything internal (parent spec §6.1). |
| D7 | Storage | `fenji-store`: SQLite in WAL mode through `sqlx` 0.9, with runtime-checked queries, embedded migrations, foreign keys on, writes in `BEGIN IMMEDIATE` transactions with a busy timeout. M3's tables are `accounts`, `invites`, `sessions`, `game_tokens` and `characters`. Invite codes, session tokens and game tokens are stored as SHA-256 hashes only. |
| D8 | What a character is | The fields of the create request that pass validation: an allow-list of keys, with type, depth and size bounds, no magic objects, and table references resolved with a guard against cycles into a plain value tree. To those the server adds its own `pid` (never reused), level 1 and the creation time. Name, vocation, skeleton and level are columns; the rest of the tree is stored as its own pickle, decoded and re-encoded into each reply (table references are numbered per packet, so a stored packet can't be spliced). For every role-list field the request lacks, W1 names the source, in this order: (1) omitted, or a constant, where the client tolerates that; (2) otherwise derived at run time from the operator's own install with `fenji-lua`, from the exe's embedded scripts, which brings `[data] client_path` into M3; (3) never committed. Reading the archives would need Oodle: a gate. Names are unique per server, by SQLite's `NOCASE`, which folds ASCII letters only. |
| D9 | Deleting | **Immediate.** There is no pending-deletion period; the reply carries values for which the client shows no "deletion pending" dialog. |
| D10 | Refusals | In `WORK`, message 270 with the client's own error codes (W1 picks them): a taken name, a bad name, too many characters, a missing character. Before `WORK`, the auth reply's close reason, using the client's own keys (`error auth-failed`, `error server-not-running` and the like). |
| D11 | Entering the world | Out of scope. `SelectActor` (10) gets a 270 refusal, so the select screen stays usable. |
| D12 | The account API | JSON over HTTP at `/api/v1`: register with an invite, sign in, sign out, get a game token, and server info. TLS comes from Caddy in front (parent spec §8). The launcher refuses plain `http://` except to a loopback address; hosts without a public domain wait for M4's deployment work (for example certificate pinning). Sign-in failures are limited per (username, address) pair and per address, which lock for a while, and per username, which only slows down, so nobody can lock another player out. Every failure gets the same message. Behind a proxy, the address comes from `X-Forwarded-For`'s last hop only when the peer is in `[server] trusted_proxies`, which is empty by default. Registration checks the invite before the username. |
| D13 | Invites | One-time codes from `fenji-server invite create [--label …] [--expires …]`, shown once; `invite revoke` and `invite list`. A player pastes the code, or an invite link `https://<server>/#invite=<code>`: the fragment carries the code, so it never reaches a server log or a link preview. `/invite` in game waits for chat (M4). |
| D14 | Launcher | `fenji-launcher` is the library: the API client, client verification against the pinned build's hash manifest, the launch command for Linux (`umu-run` in a dedicated prefix) and Windows (direct) as data, and the login path behind a trait. `fenji-launch` is a small CLI over the same library: its `play --print` gives the sandboxed runs their ticket. `apps/launcher` is the Tauri 2 shell, designed with the `impeccable` skill. Since the client never runs outside the sandbox on a developer's machine, and players wait on the gate, nothing in M3 starts the client except the runner. |
| D15 | The manifest | `crates/fenji-launcher/manifests/10015727.tsv`: the path, size and SHA-256 of each of the pinned build's 210 files. It is generated from the owner's pinned copy after that copy's exe hash matches [`pinned-build.md`](../../re/pinned-build.md), and holds hashes and names only, which parent spec §5.2 allows. The quick check compares every size and hashes `bin64`'s executables and libraries; the full check hashes everything. |
| D16 | Configuration | `fenji.toml` (parent spec §7). `[server]` gains `id`, `name`, `api_bind` and `trusted_proxies`, and `login_bind` is the gate. `[data] client_path` is read by the lobby (D8). `[store]`, `[auth]` and `[lobby]` are new. Changes need a restart in M3; hot reload comes with `/reload` (M4). |
| D17 | Limits | `[server] max_players` (default 20): an auth beyond it is refused with a close reason, using the client's own key if W1 finds a fitting one, otherwise the plain text "server full". A second login to an account already online replaces the first connection, which gets a close with `error duplicate-login`. Before auth: a 30 s deadline for the auth message, at most 64 unauthenticated connections, and at most 26 + 0x4000 bytes taken before the auth message is complete. Argon2 work runs on a blocking pool, at most 4 at a time. |
| D18 | Old tickets in the stub (#102) | `fenji-stub --no-auth-capture` keeps neither the auth payload nor the frames that carry it, for any future run with a real platform ticket. |
| D19 | v0.3.1's deferred minors | Taken now where M3's code meets them: a reference cycle guard where tables are followed (D8), the client simulator's inbound limits, the stub's `--work-pad` state, and a stale comment. The rest stay deferred. |
| D20 | Licences | The root `deny.toml` gains `[graph] targets` for the platforms Fenji builds on (Linux x86_64, Windows x86_64 MSVC and GNU, macOS arm64). That keeps the wasm-only `webpki-root-certs` out of the count, so the root list stays permissive-only. No `dirs` or `directories` crate goes into `fenji-launcher`. `apps/launcher` is its own Cargo workspace with its own `deny.toml`, which allows exactly Tauri's five MPL-2.0 crates by name (`option-ext`, `cssparser`, `cssparser-macros`, `selectors`, `dtoa-short`). MPL-2.0 is weak copyleft, not permissive, so the ledger and the morning report put the choice before the owner. `reqwest` uses `rustls` with the `ring` provider. |

### Deviations from the parent spec

| Parent spec | M3 | Why |
|---|---|---|
| §5.2: the launcher starts the client for players | No M3 build that a player would run can start the client (D2). The launcher's launch command is built and tested as data; only the runner starts the client, in the sandbox (D14) | The player path is the owner's decision (#54); rule 4 for developers |
| §5.2: the launcher can download the pinned build with DepotDownloader | Not in M3 | It moves to M4's launcher and hosting work |
| §5.1: `/invite` in game | Not in M3 (D13) | It needs chat (M4) |
| §6.4: `fenji-server` runs an ECS world | Not in M3 (D5) | Nothing in the lobby is simulated |
| §7: hot reload; `login_bind` and `world_bind` | No reload in M3; `login_bind` and the new `api_bind` (D16) | `/reload` and a world connection come with M4 |
| §8: a token-signing key in `secrets.env` | None needed | M3's tokens are random and stored only as hashes (D3, D7) |

## 3. Workstreams

| # | Workstream | Delivers | Depends on |
|---|---|---|---|
| W0 | The gate | The `Decision gate` issue for #54, filed at once (§7) | — |
| W1 | Lobby and platform reading | `docs/protocol/packets/lobby.md`, covering: <ul><li>every lobby message with its id, arguments and handler, including 370, 496 and 402;</li><li>the actor table's fields, which of them the select screen needs, and a source for each field the request lacks (D8);</li><li>error code 270 and the codes to use;</li><li>the close-reason keys;</li><li>the name rules and the number of character slots;</li><li>which add-on copy loads.</li></ul> It also reads the platform paths statically for the gate: what `--gf-sdk=steam` loads and contacts, whether Steam must run, what reaches our gate on that path (`net.luac`'s Steam branch) and whether a Steam ticket could be bound to an account; and what a stand-in for the Gameforge Client's DLL (gate #11's option b) would have to provide. Evidence follows [`../../re/README.md`](../../re/README.md): scripts by engine path and function path, native code by address and REA Evidence ID | — |
| W2 | `fenji-proto` lobby | Typed lobby messages (encode and decode, each citing W1); value-tree validation for D8; a client-like compressed pickle encoder for the bot; D19's minors in `fenji-proto` | W1 |
| W3 | `fenji-store` | The crate, its migrations and queries, tests against temporary databases | — |
| W4 | `fenji-auth` | The crate (D6), with tests | — |
| W5 | `fenji-server`: accounts | The binary, `fenji.toml`, the admin subcommands (invites, accounts), the account API | W3, W4 |
| W6 | `fenji-server`: gate and lobby | The gate listener on `fenji_proto::handshake`, the ticket check, the limits of D17, the lobby; integration tests with the client simulator, a restart included | W2, W5 |
| W7 | `fenji-launcher` and `fenji-launch` | The library and CLI of D14, the manifest of D15 | W5 (the API) |
| W8 | Runner | `run-client.sh` changes (§4) | W6, W7 |
| W9 | Rungs | The real-client runs of §5 | W8 |
| W10 | `apps/launcher` | The Tauri shell: server, account, client and Play screens. The plan may split it off if it would put the release at risk | W7 |
| W11 | The stub | D18 (#102) and D19's `--work-pad` fix | — |
| W12 | Records | `m3-summary.md`; "M3 update" notes in the parent spec's §5, §10 and §12; `login.md`; `opcodes.md`; PRIVACY.md, README and SECURITY.md | all |

The critical path is W1 → W2 → W6 → W8 → W9. W0 comes first; W3, W4 and W11 can start at once; W7 and W10 run beside W6.

## 4. Components

### `fenji-auth`

- `password`: argon2id hashing to a PHC string, with the `argon2` crate's defaults (19 MiB, 2 passes, 1 lane: OWASP's recommendation). Verification, and a dummy hash to verify against when the username is unknown, so the time taken doesn't reveal it. Passwords run from 10 to 128 characters.
- `token`: 32 random bytes in the `fenji1-` hex form, and their SHA-256 for storage. Its `Debug` output never shows the token.
- `invite`: codes of 16 characters in Crockford's base32 (80 random bits), shown in four groups, normalised on input, hashed for storage.
- `username`: 3 to 16 characters from `A–Z a–z 0–9 _ -`. The client splits `gj_uinfo` on `:`, so a colon can never pass.
- `limiter`: failure windows keyed by (username, address), by address and by username (D12), in memory, with a bounded number of keys.

### `fenji-store`

`Store::open(path)` creates or migrates the file. Each write that must be atomic is one `BEGIN IMMEDIATE` transaction:
- redeeming an invite and creating its account;
- consuming a game token;
- creating a character, with the name check and the per-account limit inside the transaction.

| Table | Columns (M3) |
|---|---|
| `accounts` | `id`, `username` (unique, `NOCASE`), `password_phc`, `created_at`, `last_sign_in_at` |
| `invites` | `code_hash` (key), `label`, `created_at`, `expires_at`, `revoked_at`, `redeemed_by` (set to null if the account is deleted), `redeemed_at` |
| `sessions` | `token_hash` (key), `account_id` (cascades), `created_at`, `expires_at` (24 h) |
| `game_tokens` | `token_hash` (key), `account_id` (cascades), `expires_at`, `used_at` |
| `characters` | `pid` (`AUTOINCREMENT`, never reused), `account_id` (cascades), `name` (unique, `NOCASE`), `vocation`, `skeleton`, `level`, `appearance` (blob), `created_at` |

Times are Unix milliseconds. The server purges expired sessions and tokens at start-up and every hour. `account delete` removes the account, and with it its characters, sessions and tokens (PRIVACY.md, "Deleting your data").

### `fenji-server`

| Module | Role |
|---|---|
| `config` | Reads and validates `fenji.toml`; refuses unknown keys and impossible values, such as a server name with a colon, and says where they are |
| `accounts` | The flows: redeem, sign in, sign out, issue and consume game tokens, with the limiter |
| `api` | The `axum` routes of D12, with request-size and time limits |
| `gate` | The listener: one task per connection running `fenji_proto::handshake::Server`; the limits of D17; the ticket check; the online-accounts registry; then the lobby loop |
| `lobby` | One function per lobby message: decoded arguments in, replies out, with store calls in between. Ids it doesn't handle are logged by id and ignored, and never close the connection. It never sends 243, 216 or 556. Tested without sockets |
| `cli` | `run` (the default), `invite create`, `invite list`, `invite revoke`, `account list` and `account delete` |

The gate keeps M2's choices: variant 3 both ways and stored Oodle packets. The auth payload's MAC address and machine fingerprint are dropped on arrival, never stored (PRIVACY.md).

### `fenji-launcher`, `fenji-launch` and `apps/launcher`

- `api`: the account API's client (`reqwest` with `rustls` and `ring`); `https://` only, except to loopback.
- `verify`: reads the manifest; runs the quick and full checks of D15; reports which files differ.
- `launch`: builds a `LaunchSpec`: program, arguments, environment and working directory. Everything `gj_*`, `gj_ticket` included, goes in the environment, where the client's setting lookup finds it ([`login.md`](../../protocol/login.md) §1), so the token stays out of command lines. The gate goes to the client as a dotted IPv4 address, because the client parses it with `inet_addr` ([`transport.md`](../../protocol/transport.md) 1.1). The launcher resolves the server's name to an A record first and says so clearly when there is none. A launcher that starts the client later must get the Wine prefix and runtime ready before it asks for the token.
- `login_path`: a `LoginPath` trait. `Uinfo` exists only with the `dev-login-path` feature (D2). It builds:
  - `gj_uinfo` as `<[server] id>:90:<[server] name>:0:<username>:<account id>` (the zone and account type M2 ran; the account id becomes the user id, which the select screen shows as the UID);
  - `gj_gates` as `<IPv4>:<port>`;
  - `gj_ticket` from the API's game-token reply.
- `settings`: the server address, username and client folder, in the platform's config directory, found without the `dirs` crate (D20). No password or session token is stored; `fenji-launch` signs in on each `play`.
- `fenji-launch` subcommands: `redeem`, `verify [--full]`, `manifest` (for developers: generates D15's file) and `play --print`, which prints the `LaunchSpec`'s `gj_*` environment. It reads passwords from a file or the environment, never from the command line.
- `apps/launcher` is its own Cargo workspace (D20), built with Vite and TypeScript, with four screens. Its webview reaches only Fenji's own commands: no shell, file-system or HTTP plugins, and a content security policy. CI builds it in a job of its own (`fmt`, `clippy`, tests, `deny`, the frontend build).

### The runner

`run-client.sh run` gains:
- `--server CONFIG`: start `fenji-server` from `target/release` inside the sandbox, in place of `fenji-stub`.
- `--server-data DIR`: bind a data directory under `re/runs/` that outlives the run, so the next run's server finds the same database (L4).
- `--launch ARGS`: run the developer build of `fenji-launch play --print ARGS` inside the sandbox once the server listens, and give the client only the `gj_*` environment it prints. `inputs.txt` records `gj_ticket` as redacted.
- `--key T:KEY` and `--type T:TEXT`: a player's key presses through XTEST, like `--click`.

## 5. Rungs

Each rung is one run through `scripts/spike/run-client.sh` (`--no-root --x11 99 --cpu-topology`, the watchdog armed), with its prediction written in `re/m3/runs.md` before it starts. An exit 3 stops all client runs and is written up for the owner. L1–L5 pass `--gf-sdk=mocked`, as M2's launcher-path runs did.

| Rung | What happens | Passes when |
|---|---|---|
| L1 | The launcher hook signs in to an existing account and gets a game token; the client starts on the launcher path | the server accepts the token once, the client reaches `WORK` and sends 402 |
| L2 | A key press at "Press Any Key" | the client sends 370, 496 and 292; the server answers 228 with no actors; the client opens character creation |
| L3 | Clicks and a typed name create a character | 304 decodes, the character is stored, 628 goes back, and the select screen shows it |
| L4 | A new run with a new server process and the same data directory | the select screen lists the character |
| L5 | Select, delete, type the confirmation | 166 decodes, the character leaves the store, 84 goes back, and creation opens again |
| L6 | L1 without `--gf-sdk=mocked` | records whether the launcher path needs any SDK flag (#54's open start-up question) |
| L7 | Optional: the server-list path with `--gf-sdk=mocked` | the lobby works there too. The server accepts the mocked SDK's ticket only when built with the non-default feature `dev-mocked-sdk`, maps it to one named developer account, and refuses to start unless `login_bind` is a loopback address. A test checks that a default build rejects it |

The runs also record which names the client looks up and which URLs it tries on the lobby screens (its `winsock` log), for PRIVACY.md. If a rung fails, we return to the reading. The run count isn't capped.

## 6. Testing

- **Unit tests** in every new crate; `fenji-store`'s run against temporary databases.
- **Golden fixtures:** the real client's create request (L3) and our replies, kept minimal. They carry no machine data, and the name in them is ours.
- **The bot** (parent spec §9): `fenji-server`'s integration tests drive the whole flow with `fenji_proto::client::Client`:
  - redeem an invite, sign in, get a token and log in, with an uncompressed and an LZ4-compressed auth payload, as the real client sends from 128 bytes up;
  - send 370 and 496, then list; create;
  - restart the server on the same database, list again, delete.

  They also cover a reused token, an expired one, a wrong account, a mocked-SDK ticket on a default build, a taken name, the character limit, the player cap, a second login to the same account and the pre-auth limits.
- **The launcher:** `fenji-launcher`'s API client against a running `fenji-server`, in `fenji-server`'s tests; verification against temporary trees; launch specs as data; a test that a default build has no `Uinfo` path.
- **CI:** the root workspace as now. `apps/launcher` runs in its own job.

## 7. Gates and stop points

| # | When | What happens |
|---|---|---|
| G1 | #54: players on the launcher path | Filed as a decision gate at once (W0), with these options: the launcher path; the Steam SDK path; a stand-in for the Gameforge Client's DLL; the mocked SDK, which #11 excludes; and deferring. W1's platform reading is added as it lands. It also notes that the AGT driver has never been checked by a run on native Windows; that answer rests on spike-0 3.11. Until it is decided, no player-facing build starts the client (D2) |
| G2 | A lobby step needs something only the official server could make, such as a signature or a key | Ground-rule-6 stop for that step: a decision-gate issue; the rest goes on |
| G3 | Any step would patch the client, circumvent a protection, load the AGT driver or use Oodle outside its approved scope. Sending hotpatch files (243), GM scripts (216) or Lua bytecode (556) counts as patching | Stop and file a gate |
| — | Runner exit 3 | All client runs stop; the event is written up for the owner |

## 8. Process

- One plan: `docs/superpowers/plans/2026-10-12-m3-login-lobby.md`. Each task becomes a `Task` issue in milestone M3.
- Execution is inline by the controller, test-first. Before each squash-merge into `dev`, a fresh reviewer reviews that PR clean (Critical and Important fixed). One final review of the whole milestone, then one fix pass.
- **Clean room:** RE work (scripts, Ghidra, REA) and every client run go only to sessions and agents that have never read material rejected in [`../../re/sources.md`](../../re/sources.md).
- `docs/` holds prose and tables only, with no decompiled code. Tags: **Observed**, **Inferred**, **Unknown**.

## 9. Done when

1. **Accounts:** invites, registration, sign-in with the limiter, game tokens; tested.
2. **The ticket handoff:** the launcher's code signs in, gets a game token and builds the `LaunchSpec`; the runner starts the client with it in the sandbox; the server takes the token once (L1).
3. **The lobby in the real client:** L2, L3 and L5 pass, and the watchdog stays clean.
4. **Persistence:** L4 passes, and the bot's restart test runs in CI.
5. **The launcher:** `fenji-launcher`, `fenji-launch` and `apps/launcher` build and pass their tests; verification works against the pinned copy; a default build can't start the client (D2).
6. **Records:**
   - `docs/protocol/packets/lobby.md` with evidence: script paths, plus REA Evidence IDs for native findings per the norm;
   - `opcodes.md`'s script-message table and `login.md`'s launcher-path section;
   - `docs/re/m3-summary.md`, with L6's outcome;
   - "M3 update" notes in the parent spec's §5, §10 and §12; §12's M3 column answers the ticket question;
   - PRIVACY.md (the lobby's lookups and URLs, the connection's threat model), README (status; v0.4.0 isn't for players) and SECURITY.md ("nothing meant to face the internet" stays true until M4's TLS);
   - the gate issue filed and updated with W1's platform reading; #54 answered as far as RE goes; #102 closed.

Release: v0.4.0, under the owner's pre-approval (ledger, Ruling R0).

## 10. Out of scope

- Entering the world, a second connection, the ECS world, BRP and the time-sync reply (#91): M4.
- Hot reload, chat commands and `/invite` in game: M4.
- The reference deployment (Caddy, systemd), TLS for hosts without a public domain, and DepotDownloader in the launcher: M4.
- Character rename, rebirth, slot changes and the shop on the select screen.
- Starting the client outside the sandbox, any change to the client, and any run with a real platform SDK.
