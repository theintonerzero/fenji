# M3: summary

M3, login and lobby, turned the stub's handshake into a server people can sign in to:
- A host invites players.
- A player redeems an invite, signs in, and gets a single-use ticket.
- The real, unmodified client logs in to `fenji-server` with that ticket. It lists its characters, creates one and deletes one.
- A character survives a restart of the server.

The owner decided gate #114 during M3. Players use the same launcher path, and `fenji-launch play` starts the client on Linux, as a player's launcher does.

**Status (2026-10-13):** Done-when items 1–4 and 6 are met. Item 5 is met as amended: the owner moved the desktop shell to M4 (#146). Release: v0.4.0, pre-approved by the owner for when the final review is clean and CI is green (ledger, Ruling R0).

What M3 built:
- **`fenji-auth` (#129):**
  - argon2id password hashes;
  - game tokens, session tokens and invite codes, stored only as SHA-256 hashes;
  - the username and password rules;
  - the sign-in limiter.
- **`fenji-store` (#130):** SQLite in WAL mode through `sqlx`, holding accounts, invites, sessions, game tokens and characters.
- **`fenji-server` (#132, #133):**
  - `fenji.toml`;
  - the account API at `/api/v1`;
  - the admin commands for invites and accounts;
  - the gate: the handshake, the ticket check, and the limits on players, waiting connections and sizes;
  - the lobby: list, create and delete.
- **`fenji-proto`'s lobby (#131):** typed lobby messages, the checks on what a client asks to store, and a client-like encoder for the test bot.
- **`fenji-launcher` and `fenji-launch` (#134, #140):** sign in, check the client against the pinned build's manifest (build 10015727, 210 files), and take a ticket. On Linux they also start the client through `umu-run`, in the launcher's own prefix. Every other system is refused.
- **The runners:**
  - `run-client.sh` runs the client against `fenji-server` with the launcher's ticket and key presses (#136).
  - `run-bare.sh` is the guarded bare-metal runner (#144).
- **The stub's ticket hygiene (#145):** `--no-auth-capture` keeps no auth payload (#102).

The documents:
- [`../protocol/packets/lobby.md`](../protocol/packets/lobby.md) covers:
  - every lobby message;
  - the actor table, the create request and its result, and delete;
  - errors and close reasons;
  - what Fenji sends;
  - the platform paths (§12);
  - the runs (§14).
- [`../protocol/login.md`](../protocol/login.md) §7: the launcher path with a Fenji ticket.
- [`../protocol/opcodes.md`](../protocol/opcodes.md): the lobby's script messages.
- [`README.md`](README.md): client runs against `fenji-server`, and bare-metal runs.
- The root README, PRIVACY.md and SECURITY.md: what M3 changes for players and hosts. The game connection is readable and unauthenticated, so no password crosses it. The server's account API speaks plain HTTP, and `fenji-launch` refuses `http://` except to this machine. They also cover umu's own requests.

## Open questions answered

| From | Question | Answer | Where |
|---|---|---|---|
| Parent spec §12 | What the ticket is and how Fenji issues it | **Answered and run.** A Fenji game token: `fenji1-` and 64 hex characters. It is single-use, valid for 60 s, bound to one account and stored as its SHA-256. It reaches the client in its environment as `gj_ticket`, and the client sends it as it is: its hex-decoding step can't run in this build. | M3 spec D3; `lobby.md` 12.7; `login.md` §7 |
| #54, gate #114 | Which login path players use | **Decided by the owner, 2026-10-13: the launcher path** (option a). The client is verified against the pinned manifest, then started unmodified with `gj_uinfo`, `gj_gates` and a single-use Fenji ticket. No platform credentials are involved, and ownership rests on the host's vetting (ground rule 1). | `lobby.md` §12; #114 |
| #54 | Whether the launcher path needs an SDK flag | **No.** In the sandbox, m3-l6 started and logged in with no `--gf-sdk` flag. On bare metal, bm-2 to bm-6 signed in the same way, through `umu-run`, on the real GPU. | `lobby.md` 12.6, §14; below |
| `lobby.md` §13 | What only a real-client run could settle | **Questions 7, 9, 11 and 15 settled; 1 and 5 in part.** The empty list opens creation. A real 304 is 758 bytes, sent LZ4-compressed. Nothing visibly broke when 495 was never sent. The client starts with no SDK flag. | `lobby.md` §13–14 |
| `login.md` §1, spike-0 8.2, 4.34–4.35 | Earlier readings of the platform paths | **Corrected.** The scripts' Steam branch is dead in this build, the platform DLL's configuration search runs in `DllMain`, and `121.5.97.203:8001` is the Chinese gate fallback. | `lobby.md` 12.8 |

## Deviations from the spec

| # | Deviation | How it ended |
|---|---|---|
| D1 | The owner decided gate #114 (plan Amendment B): the launcher path for players | The `dev-login-path` feature was removed and `Uinfo` is in every build (#134). `fenji-launch play` starts the client on Linux and refuses every other system, `--print` included, until the AGT driver is read for native Windows (#140). |
| D2 | Guarded bare-metal play tests on the Linux game box (Amendment B) | `run-bare.sh` enforces every guard (#144); see "Bare-metal runs" below. |
| D3 | Task 11, the desktop shell (`apps/launcher`) | Moved to M4 by the owner's decision: v0.4.0 ships with the CLI launcher. #146 holds the owner's brief, and the owner approved the screen designs, whose tokens become Fenji's one brand identity. The host's console is a new request for M4 (#147). |
| D4 | L7, the optional server-list rung with the mocked SDK | Not run, by a ledgered ruling: L1–L6 cover the handoff, list, create, persistence, delete and the path with no flag. Players never use the mocked SDK's server list. |
| D5 | Spec D8's value tree and its cycle guard | Typed scalars instead: the create request carries only scalars (ruling P1, plan Amendment A). |
| D6 | Spec D16's `[data] client_path` | Parsed but not read in M3: `prop_sys_equips` stays empty, so nothing comes from the operator's install (ruling P4). |
| D7 | Spec D17's 26 + 0x4000 bytes before the auth message | `PRE_AUTH_CAP` is 26 + 4 + 0x4000 + 512, to allow for the header, the frame lengths and time syncs (Amendment A). |
| D8 | The `invites` table's key | `id`, with `code_hash` unique (Amendment A). |
| D9 | Spec §4's limiter "per username" | Pairs of a username's hash and a network lock after 5 tries in 15 minutes, and networks (IPv6 by /64) after 20. A username only slows down, counted in one of 4096 buckets keyed per process, so the limiter never learns whether an account exists and nobody can lock another player out by knowing their username, though players behind one shared address or /64 share its lock (Amendment A, after six security reviews). |

## Done when

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | Accounts | **Met.** Invites, registration, sign-in with the limiter, and game tokens, all with unit tests and the server's integration tests. | #129, #130, #132 |
| 2 | The ticket handoff | **Met.** In L1 (m3-l1), `fenji-launch play --print` signed in, took a game token and printed the launcher path's `gj_*` pairs. The client logged in with them, and the server took the token once. On bare metal, `fenji-launch play` started the client itself, through `umu-run` (bm-2 to bm-6). | `lobby.md` §14; #136, #140 |
| 3 | The lobby in the real client | **Met.** L2, L3 and L5: list, create and delete; on bare metal too (bm-4, bm-5). The watchdog showed no sign of the AGT driver in any run; bm-1's exit 3 was a proven false positive (below). In m3-l3a and m3-l3b its rule S2 didn't arm (#142, fixed in #143), and both `winedevice.exe` had started before the client. | `lobby.md` §14 |
| 4 | Persistence | **Met.** L4 (m3-l4) listed a character from the run before after a new server process started, and so did bm-5 on bare metal. The bot's restart test runs in CI. | `lobby.md` §14; `crates/fenji-server/tests/lobby.rs` |
| 5 | The launcher (as amended) | **Met as amended.** `fenji-launcher` and `fenji-launch` build and pass their tests. Verification works against the pinned copy. `play` starts the client on Linux only. The desktop shell moved to M4 (#146). | #134, #140 |
| 6 | Records | **Met:** this document; `lobby.md` with REA Evidence IDs; `opcodes.md`; `login.md` §7; "M3 update" notes in the parent spec's §5.1–§5.3 and §10; §12's M3 column; PRIVACY.md, README and SECURITY.md. Gate #114 is decided, with the platform reading on it, #54 is answered as far as RE goes, and #102 is closed. | #135, #137, #141; this PR |

## Bare-metal runs

`run-bare.sh` ran on the owner's Linux game box under every guard of CLAUDE.md's "Bare-metal client runs":
- `umu-run` in a dedicated prefix;
- a reflink copy of the pinned copy, never the Steam library, with Steam not running;
- a local `fenji-server`;
- a namespace with loopback only and empty `/run`, `/tmp` and `/dev/shm`, bar the audio sockets (PipeWire, Pulse) and the display's, bound back. Sockets elsewhere in the real filesystem stay reachable, such as the input-method daemon's in `~/.cache/ibus` (#148);
- the AGT driver watchdog armed.

There was no SDK flag in any run. The lobby works end to end on bare metal: sign-in through `fenji-launch play` and `umu-run`, the list, create, a character surviving a server restart, and delete (bm-4, bm-5).

bm-4 to bm-6 ran with `--desktop`, on the session's own display. The owner, who ran them, accepted that exposure for hands-on test runs on 2026-10-13. There the client shares the session's X server: it can read and type into other X11 windows, read and replace the bridged clipboard, and its XTEST input may reach native windows. Its network stayed loopback-only, but it was less isolated than on a nested display. Automated runs use a dedicated, GPU-capable nested display (#144).

| Run | What happened | Watchdog |
|---|---|---|
| bm-1 | Stopped before the client got going. | **Exit 3, proven a non-AGT false positive.** GE-Proton's own launch shim, `umu.exe`, registered GE-Proton's Discord bridge service 37 ms before it created the client. That service is user-mode (`bridge.exe --service`, type 0x10) and was never started. No `.sys` was written, no driver loaded beyond Wine's own, and the client touched no `PE` resource. `run-bare.sh setup` had built the prefix with `umu-run createprefix`, which never runs the shim, so the service wasn't in the baseline. Setup now primes the prefix through Proton's launcher, as `run-client.sh` setup already did (#144). |
| bm-2 | Signed in on the real GPU: "in the lobby" for `tester`. Then the client died at its first frame: its X connection broke. The display was the sandbox's `:99`, which keeps GPU acceleration off by design, so that a sandboxed client gets no GPU device, and the real GPU's frames can't be shown there. Bare runs now use a GPU-capable nested display of their own, or `--desktop` (#144). | No sign. |
| bm-3 | The same command as bm-2, on `:99` again, with the same result. | No sign. |
| bm-4 | `--desktop`, on the real GPU. The client signed in, the empty list opened character creation, and it created "Mai" (304). Entering the world was refused, as M3 intends (10, then 270), and the select screen stayed usable. The owner quit; exit 0. | No sign. |
| bm-5 | `--desktop`, a new server process on the same database. The list showed "Mai" after the restart. The client created "test" and deleted it again (304, then 166). The owner quit; exit 0. The database then held "Mai" alone. | No sign. |
| bm-6 | `--desktop`. The list showed "Mai". Entering the world was refused each of the eight times it was tried (10, then 270), and the select screen stayed usable. The owner quit; exit 0. | No sign. |

## Gates and the owner's rulings

| Gate | Outcome |
|---|---|
| #114: players on the launcher path | **Decided 2026-10-13: option (a).** Still open: whether the AGT driver loads on native Windows on this path. A static reading comes first, and its result goes to the owner before Windows players get a way to start the client (ground rule 6). |
| #11: the mocked SDK | Used in sandboxed developer runs only (L1–L5). L6 and every bare-metal run passed no SDK flag. |
| G2: something only the official server could make | **Not hit.** |
| G3: patching, a protection, the AGT driver, Oodle out of scope | **Not hit.** Fenji sends no hotpatch (243), GM script (216) or Lua bytecode (556); the server refuses to build them. |
| Runner exit 3 | **Once, in bm-1,** proven a non-AGT false positive (above). Under the owner's M2 rule, runs went on only after the proof and the fix. |

The owner's rulings during M3:
- **v0.4.0:** pre-approved once the final review is clean and CI is green (R0).
- **Gate #114:** option (a).
- **Bare-metal tests:** guarded bare-metal play tests are allowed on the Linux game box only, never with `--gf-sdk=mocked`.
- **The launcher:** v0.4.0 ships with the CLI launcher. The desktop launcher follows in M4 from the owner's brief (#146), in the design the owner approved, whose tokens become Fenji's one brand identity.
- **The bare-metal display (2026-10-13):** `--desktop` is allowed for hands-on test runs, with its exposure documented; automated runs use a nested display. These runners are developer tools: players run the game on their own desktop.

Findings M4 inherits:
- **Entering the world:** `SelectActor` (10) gets a 270 refusal, so the select screen stays usable (spec D11).
- **The AGT driver on native Windows** on the launcher path is unread (above).
- **TLS:** the server's account API speaks plain HTTP, and `fenji-launch` refuses `http://` except to this machine. Hosting for friends elsewhere needs a TLS proxy (Caddy), which is M4's deployment work, as are hosts without a domain.
- **Time syncs in the world (#91):** the client's time-sync messages need answers once a character is in the world.
- **Outfits:** `prop_sys_equips` is empty in M3 (ruling P4). The outfit shown at creation would come from the client's own tables in the operator's install, which is M4's to decide.
- **The world's first messages:** a successful 10 is answered by the world, which may queue the player (605, 728) and then loads the scene (772; `lobby.md` 10.2, inferred).
- **The desktop launcher and the host's console** (#146, #147), with download and repair through DepotDownloader for Steam owners.
- **The lobby's still-open questions** are listed in `lobby.md` §13.
