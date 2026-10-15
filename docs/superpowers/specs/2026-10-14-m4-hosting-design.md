# M4 hosting: the host console, maintenance mode and the reference deployment — Design Spec

**Status:** written on the night of 2026-10-14 under the owner's pre-approval of new specs and plans ("I pre approve all new versions and plans/specs etc"). Every choice the owner hasn't made is a ruling, H1 to H13 in §7, with what it costs if it's wrong, so the owner can reverse any of them.

**Parent:** the [M4 umbrella spec](2026-10-13-m4-social-sandbox-design.md) §3.2 (hosting), §4 (what the pieces share) and §6; the [Fenji design spec](2026-10-11-fenji-design.md) §6.6 (admin), §6.7 (capacity), §7 (configuration) and §8 (deployment). The issues are #147 (the host console) and two server items of #174.

## 1. Purpose

Hosting is done when someone who isn't a developer can run a Fenji server for the friends they invite:
- on their own computer (Linux, Windows or macOS) by running a start script, or on a VPS by following the write-up alone;
- in a console with live logs above and a command line below, like a Minecraft server's, on their own computer or over SSH;
- reachable by friends on the same network, over the internet or over a VPN, with or without a domain;
- with maintenance mode, the server facts the launcher shows, and nightly backups.

Nothing here touches the game client. Every task is repository-only.

## 2. Constraints

- **The ground rules and the umbrella spec's constraints hold.** In particular, Fenji never handles platform credentials, and a server is for 20 invited players (parent §6.7): one process, one world, one SQLite file.
- **No telemetry, and no calls home.** The server and its console make no network connection of their own: no update check, no "what is my IP" service. A host types the addresses.
- **Dependencies** stay under `deny.toml`'s licences. Hosting adds `ratatui` and `crossterm` (MIT), `rustls`, `tokio-rustls` and `rcgen` (MIT or Apache-2.0, with `ring`, already in the graph through `fenji-launcher`), and `tracing-appender` (MIT). The plan pins versions from their documentation at the time (context7).
- **The console's look** is the approved design (2026-10-13): `DESIGN.md`'s "The host console" and its colour tokens. `DESIGN.md` is committed with the launcher's spec (#146), so §3.2 copies what the console needs.
- **Order with the world's plan (H13).** The tasks that change `crates/fenji-server/src/world/` start after the world plan's Task 16 has merged, so the two plans never edit the world at once. The rest can start at once.

## 3. Design

### 3.1 Two ways into the console (H4)

The server has one console service: it runs commands (§3.3) and streams the log and the server's status. Three front ends use it.
- **In the terminal that started the server.** When `fenji-server` (or `fenji-server run`) has a terminal on both stdin and stdout, `TERM` isn't `dumb` and `--plain` isn't given, the console takes over that terminal (§3.2). This is what the start scripts open, on all three systems, and what a host sees over SSH (in `tmux` or `screen`, so the server outlives the connection). Ctrl-C stops the server as `stop` does, gracefully.
- **Plain lines.** Otherwise the server logs plain lines to stderr, as now, and reads commands as lines from stdin until it ends, printing each answer to stdout. Under systemd stdin is `/dev/null`, so it reads nothing. `--plain` asks for this mode in a terminal too, for screen readers and simple terminals.
- **Attached to a running server (Linux and macOS).** `fenji-server console` connects to the server's console socket (§3.5) and shows the same console. Ctrl-C or `exit` leaves the console and the server runs on; `stop` stops the server. `fenji-server console --run "<command>"` runs one command, prints its answer and exits with 0, or 1 if it failed, for scripts such as the update steps (§3.13). `--plain` works here too.

On Windows the console runs in its own window only. A host who runs the server as a Windows service uses the subcommands (`invite`, `account`, `backup`).

### 3.2 What the console shows

From the approved design, laid out in terminal rows:
- **Title bar** (one row): `fenji-server — <server name>`, in text-2 on ink-900.
- **Status strip** (one row, ink-850): the state word in its colour (`online` in ok, `maintenance` in warn, `stopping` in danger), the server's name in gold, then facts separated by a faint ` · `: the version, the uptime, `3 of 20 online`, the maintenance message while it's on, and the gate's and the API's ports.
- **The log** fills the middle, anchored to the bottom, in three columns: the time (`HH:MM:SS`, faint), the level (INFO muted, WARN warn, ERROR danger) and the message. A WARN or ERROR line's message is in the text colour. Fields that name people (characters and accounts) are gold. PgUp and PgDn scroll back through the last 5,000 lines.
- **The command palette** opens above the prompt while the first word is typed, or on Tab: a square ink-850 list of the matching commands with a short description, the selected row on ink-800. ↑ ↓ choose, Tab completes, Enter runs and Esc closes it.
- **The prompt row** (one row, a hairline row above it): a gold `> `, the line being typed with a block cursor, and a faint hint at the right, `Tab completes · ↑ ↓ choose · Enter runs`, dropped when the terminal is narrow. With the palette closed, ↑ and ↓ walk this session's last 100 commands.

The colours are DESIGN.md's tokens: ink-950 `#0c0c0f` (the log's ground), ink-900 `#111115`, ink-850 `#16161b`, ink-800 `#1c1c22`, text `#f2efe9`, text-2 `#c9c2b7`, muted `#a8a196`, faint `#8c867c`, gold-300 `#f2b365`, ok `#7fd38f`, warn `#e8b04a`, danger `#f26d7d`. Ember isn't used: the console has no primary action.
- They are exact where the terminal says it has 24-bit colour (`COLORTERM` is `truecolor` or `24bit`), and the nearest of 256 colours elsewhere.
- With `NO_COLOR` set there are no colours: WARN and ERROR lines are bold, and the selected palette row is in reverse video.
- Fonts are the terminal's own. The write-up suggests Geist Mono (OFL).
- A terminal smaller than 60 × 15 shows a one-line note asking for a bigger window. The server keeps running.
- On any exit, a panic included, the console gives the terminal back (main screen, cursor, cooked mode) before it prints anything.

### 3.3 The commands (H5)

One parser, `fenji_server::commands`, knows where a line comes from: the game's chat (an admin's `!` line) or the console. The umbrella spec's §4.3 fixes the shared four; the console adds its own.

| Command | In chat (`!`) | In the console | What it does |
|---|---|---|---|
| `tp <name> [<name>]` | yes | two names only | Chat: takes the admin to a player, or the first player to the second. The console has no character, so it takes the first to the second. |
| `kick <name>` | yes | yes | Lets the player go with the kick reason. |
| `announce <text>` | yes | yes | The marquee, for everyone in the game. |
| `invite [label]` | yes | yes | A new invite link (§3.11). |
| `online` | — | yes | Who is connected: the account, the character or "lobby", the scene, and since when. Never an IP address. |
| `accounts` | — | yes | The accounts, as `fenji-server account list` shows them. |
| `invites` | — | yes | The invites, as `invite list` shows them (never their codes). |
| `revoke <id>` | — | yes | Revokes an unused invite, as `invite revoke`. |
| `maintenance [on [<duration>] [<message>] \| off]` | — | yes | §3.9. Alone, it shows the state. |
| `backup` | — | yes | §3.12. |
| `stop` | — | yes | Stops the server gracefully, as SIGTERM does. |
| `help [<command>]` | — | yes | The commands, or one command's usage. |

- A console-only word in chat is answered as an unknown command, as now. `tp` with one name in the console is answered with its usage, `tp <name> <name>`.
- The four shared commands run in the world, through the same code as an admin's: `world/admin.rs` gains a console caller, which the world reaches through a new `WorldIn::Console { order, reply }` with a one-shot channel for the answer. `invite` still goes through the services task.
- The console's own commands run in the console service, against the store, the online list, the maintenance state, the backup and the stop signal.
- Every command the console runs is logged, as admins' chat commands are, with `by = "console"`.

### 3.4 The log (H10)

- **Without the full-screen console**, the log goes to stderr as now; systemd's journal keeps it.
- **With the full-screen console**, the log goes to its pane and to a file, `data/logs/fenji.log`, rolled daily with 14 days kept. The pane is gone when its window closes, so the file is what a host attaches to a bug report.
- **Attached consoles** get every line as it's logged, after the last 500 lines at attach time.
- Logging never waits on a console. Each console has a bounded queue, and one that falls behind loses lines and is told how many: `… 37 lines skipped`.
- The level stays `info` unless `RUST_LOG` says otherwise.

### 3.5 Attaching (Linux and macOS)

- The server listens on a Unix socket, `console.sock`, in its data folder (the database's directory), created readable and writable by its own user only (0600). Over SSH a host attaches as that user: `sudo -u fenji fenji-server --config /etc/fenji/fenji.toml console` in the reference deployment.
- **One server per data folder.** At start, a `console.sock` that a server answers on means another server is using this data folder: the new one refuses to start and says so. One nobody answers on is stale and is replaced.
- **The protocol** is JSON lines.
  - The server speaks first, with its version and the protocol's.
  - Then it sends log lines, status changes and answers, each answer tagged with its command's number.
  - A console sends numbered command lines.
  - A console of another protocol version is told so and closed.
- **Limits:** at most 4 consoles at once and lines of at most 4 KiB. The socket is local and owner-only, but the server still bounds what it reads.
- Windows has no console socket in M4.

### 3.6 The first run (H9)

- `fenji-server` with no config at the default path (`fenji.toml` in the working directory) writes one and creates `data/`, then starts.
  - The config is `fenji.example.toml`, built into the binary, with the database at `data/fenji.db`.
  - It binds loopback, so only this computer can join until the host says otherwise.
- The console's first lines say what happened and what's next: the files it wrote; `invite` makes the first invite link; friends on other computers need `public_host` and the steps in `docs/hosting.md`.
- `fenji-server init [--config <path>]` writes the same file at a path, for a VPS. A config path given with `--config` that doesn't exist is an error with that hint, so that a mistyped path under systemd never starts a server with a fresh, empty database.
- The example config stays the one source of every key's description.

### 3.7 Start scripts and release archives (H3)

- **Start scripts:**
  - `start-server.sh` (Linux): changes to its own folder and runs `./fenji-server "$@"`.
  - `start-server.command` (macOS): the same, and opens in Terminal from Finder.
  - `start-server.bat` (Windows): changes to its own folder, runs `fenji-server.exe %*`, and pauses on a non-zero exit so the window stays open with the error.
- **Archives:** `fenji-server-<version>-<target>.tar.gz`, or `.zip` for Windows, each with:
  - the binary and its system's start script;
  - `fenji.example.toml`;
  - a short `README.txt` (run the start script, type `help`, read `docs/hosting.md`);
  - `LICENSE`, and `SOURCE.txt` with the tag's source code URL (the AGPL's offer).

  A `SHA256SUMS` file covers all the archives.
- **Targets:**
  - `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`: static, for any distribution and for ARM servers;
  - `x86_64-pc-windows-msvc`;
  - `aarch64-apple-darwin`.

  `deny.toml`'s `[graph] targets` gains the two musl targets.
- **The workflow:** `.github/workflows/release.yml` builds each target on a native GitHub runner when a `v*` tag is pushed, and attaches the archives to that tag's draft release. Run by hand (`workflow_dispatch`), it builds everything and keeps the archives as workflow artifacts only, as a dry run. `docs/development.md`'s release steps gain the wait for it.
- **Unsigned binaries:** Windows SmartScreen and macOS Gatekeeper warn about them, and the write-up shows how to get past each. Signing the server is out of M4. The launcher's signing key is the owner's decision (#146).

### 3.8 Server facts and who is online (umbrella §4.1; H7, H11)

`GET /api/v1/server` keeps its fields and adds:
- `players`: `{ "online": n, "max": m }`. `n` counts connected accounts, in the lobby or the world (`Online::count`), and `m` is `max_players`.
- `state`: `"online"` or `"maintenance"`, and `maintenance`: `{ "message", "since", "until" }` while it's on, with times in Unix milliseconds and `until` possibly `null`.
- `source`: the URL of the running server's source code. It comes from `[server] source_url`, which defaults to this repository at this version's tag. A host who changes Fenji points it at their changes: the AGPL's §13 asks them to offer that source to their players.

A server that doesn't answer is offline. The launcher concludes that itself.

`GET /api/v1/players`, for a signed-in session only (401 otherwise), returns `{ "players": [{ "name": "Mai" }, …] }`: the characters in the world, sorted by name. The world publishes the names on a `watch` channel when someone enters or leaves, not every tick. PRIVACY.md gains a line: signed-in players see the names of the characters in the world.

### 3.9 Maintenance mode (umbrella §4.2; H6)

- `maintenance on [<duration>] [<message>]` turns it on.
  - The duration (`30m`, `2h`, `1d`: `invite create`'s syntax) is when the host expects to be back. It's shown to players, and doesn't end maintenance by itself.
  - The message is plain text of at most 200 characters. The default is "Back soon", which reads well after the words players see before it, such as the launcher's "Moonward is in maintenance: " (the plan's Task 8 ruled it).
- **While it's on:**
  - Signing in and asking for a game ticket answer 503 with `{"error": "maintenance", "message", "until"}`, except for admins (`[access].admins`), so the host can check the server before opening it again.
  - Redeeming an invite answers the same 503 for everyone, so that the invite isn't spent on an account that can't sign in.
  - Players already in the game stay, until the host kicks them or stops the server.
- **The world announces it:** `Maintenance: <message>` when it starts, and `Maintenance is over.` when it ends.
- **It persists.** The state is kept in the database (a new `server_state` table), so a host can turn maintenance on, update and restart, check, and only then turn it off.
- The status strip shows it, and `maintenance` alone shows the state, the message and the times.
- `fenji-launcher`'s API client turns the new error into a message carrying the host's text, which `fenji-launch` prints. The desktop launcher (#146) shows it.

### 3.10 TLS without a domain (H1)

Hosts with a domain keep parent §8's path: Caddy in front of the account API, with a Let's Encrypt certificate. For everyone else:
- **The server serves TLS itself.** With `[server] tls = "self"`, the server serves the account API over TLS on `api_bind`, with its own key: ECDSA P-256, made on first use and kept in `data/tls/`, the key readable by the server's user only. The certificate is self-signed and names `public_host`.
- **Invite links carry the key's pin:** `https://<public_host>:<API port>/#invite=<code>&pin=<pin>`, where the pin is the base64url SHA-256 of the key's SubjectPublicKeyInfo. The fragment never reaches a server, as the code's doesn't. The console also shows a server link without an invite, `…/#pin=<pin>`, for players who already have an account.
- **The launcher pins.**
  - `fenji-launcher`'s API client accepts, for an address with a pin, exactly the certificate with that key, whatever its name and issuer, and still checks the handshake's signature with it.
  - The pin is saved with the server's address.
  - A different key is refused with "this server's key has changed: ask your host for a new link", and plain http stays refused off this machine (M3 D12).
  - An address without a pin uses the web's usual certificate authorities, as now.
- **`tls` unset** (the default) serves plain http on `api_bind`: for loopback, or behind a proxy that holds the certificate.
- **The key is the server's identity.** The write-up says to keep `data/tls/` with the backups. A host who loses it sends their players a new server link.

**Why a pin, and not a free DNS name or an IP certificate:** both need ports 80 or 443 open to the internet and a stable address. Both fail on a VPN, behind carrier-grade NAT and on a LAN. A pin works in all of those, and on a home computer whose address changes. It asks for no new trust: the invite link already travels over a channel the host trusts, and already carries a secret.

**Opening the link in a browser:** a browser shows a certificate warning for a self-signed server, and the API has no page at `/` anyway. Links are for pasting into the launcher, and the write-up says so. The desktop launcher may add a link handler (#146).

### 3.11 Addresses and invite links (#174)

- `public_host`'s description, in `config.rs` and the example: the address players' game clients connect to (the gate). The account API's address is `public_api`.
- Invite links start with, in order:
  1. `public_api`, when it's set;
  2. with `tls = "self"`, `https://<public_host>:<api_bind's port>`, with the pin;
  3. `http://<api_bind>`, only when both `api_bind` and `public_host` are loopback.

  Otherwise the server warns at start that invite links need `public_api` or `tls = "self"`. `invite` then prints the bare code, with that hint.

### 3.12 Backups (H8)

- `fenji-server backup [--keep <n>]` and the console's `backup` write a consistent copy of the database with SQLite's `VACUUM INTO`, which is safe while the server runs. The copy goes to `data/backups/fenji-<UTC timestamp>.db`. Then the oldest copies beyond `--keep` (default 14) are deleted, and the new file's path is printed.
- The reference deployment's systemd timer runs it nightly. The write-up shows Task Scheduler on Windows and cron or launchd on macOS.
- **Restoring:** stop the server, put the copy in place of `fenji.db` (removing `fenji.db-wal` and `fenji.db-shm`), and start it.

### 3.13 The write-up (H2, H12)

`docs/hosting.md` is for hosts, not developers. `deploy/` holds what it installs: the systemd units (the server; the backup service and its timer), a `Caddyfile` for hosts with a domain, and the firewall commands. Its sections:
1. **Before you start.** Every player needs their own copy of the game. Trust goes both ways (PRIVACY.md). The law (the README).
2. **On your own computer.** Download, run the start script, type `invite`, send the link.
3. **Letting friends in.**
   - On the same network.
   - Over the internet with open ports: router forwarding, the firewall, `tls = "self"`. It says plainly what an open game port exposes: the game connection is readable and authenticates nobody ([`transport.md`](../../protocol/transport.md) 3.9, PRIVACY.md).
   - Over a VPN (WireGuard, Tailscale, ZeroTier): the private option, and the way past carrier-grade NAT.
4. **On a VPS: the reference deployment.** Parent §8: a `fenji` user, systemd with its hardening, `tls = "self"` or Caddy with a domain, `ufw` with the gate's port and either the API's port or 443, the backup timer, and attaching the console.
5. **Updating.** `maintenance on 15m Updating Fenji`, `console --run stop`, replace the binary, start, check, then `maintenance off`.
6. **Backups and restoring.**
7. **Troubleshooting:** ports, SmartScreen and Gatekeeper, "a server is already using this data folder", "this server's key has changed".

**The default path for the internet (H2)** is open ports. Parent §8 designed it that way, and players install nothing more. The write-up recommends a VPN to anyone who wants their game traffic private.

**Verifying the write-up (H12):** a fresh Debian container with systemd (podman), set up by following `docs/hosting.md` alone, with `tls = "self"`, the backup timer and an attached console. A real VPS, and friends on other machines, are M4's acceptance session (umbrella §5).

## 4. Testing

- **Commands:** the parser for both sources; console-only words refused in chat; `tp` with one name refused in the console; each console command against a server started on port 0, and the world's four through `WorldIn::Console`.
- **The full-screen console:** `ratatui`'s test backend at 80 × 24 and 120 × 40, covering:
  - the title bar, the status strip, the log's columns and colours, and the palette open and closed;
  - the too-small note, and `NO_COLOR`;
  - the keys: Tab, the arrows with the palette open and closed, Esc, PgUp and PgDn.
- **Plain mode:** commands from a pipe, answers on stdout, the log on stderr.
- **Attaching:**
  - the hello and the backlog, a command and its answer, a status change;
  - a stale socket replaced, and a second server on the same data folder refused;
  - a fifth console refused, an over-long line refused, and a console that falls behind told what it lost.
- **The first run:** a folder with no config gets `fenji.toml`, which parses, and `data/`; a missing `--config` path is an error; `init` writes the file and won't overwrite one.
- **Server facts and maintenance:**
  - the JSON shapes;
  - `players` with and without a session;
  - the three refusals and the admins' exemption;
  - maintenance surviving a restart;
  - the two announcements.
- **TLS:**
  - with `tls = "self"`, `fenji-launcher` signs in with the right pin;
  - a wrong pin, and no pin, are refused;
  - the key is kept across restarts, and is owner-only on Unix.
- **Invite links:** a table test of §3.11's cases.
- **Backups:** a copy taken while a writer works passes `PRAGMA integrity_check`, and `--keep` prunes the oldest.
- **Releases:** a `workflow_dispatch` dry run on the pull request's branch builds all four archives, and a script checks what each holds.
- **The write-up:** the container run of §3.13.

## 5. Done when

The umbrella spec's §3.2, made concrete:
- The reference deployment works in a fresh Debian container by following `docs/hosting.md` alone (H12): systemd, `tls = "self"` or Caddy, the firewall commands, nightly backups.
- Hosts without a domain have a documented path: `tls = "self"` and pinned links (H1).
- The console runs `fenji-server`, with live logs above and a command line below:
  - in its own terminal on Linux, Windows and macOS, and over SSH;
  - attached to a systemd service on Linux;
  - with §3.3's commands.
- A first run without a config writes one; the start scripts and archives come out of the release workflow's dry run.
- Maintenance mode reaches players through the API and in the game, and `GET /api/v1/server` carries §3.8's facts.
- `#174`'s two server items are fixed (§3.11).

## 6. Out of scope

- Reloading the config while the server runs (`reload`; parent §7). Changes still need a restart.
- A console socket on Windows, and a Windows service or a launchd job. The write-up runs the server in the foreground on both.
- Signing the server's binaries, a web admin page, metrics, and container images (parent §8: no Docker).
- Protection against floods beyond the gate's caps (M2, M3) and the host's firewall.

## 7. Rulings

Each is the controller's, made under the owner's pre-approval, and recorded so the owner can reverse it.

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| H1 | Hosts without a domain use the server's own TLS key, pinned through invite links (§3.10). | It works on a LAN, a VPN, behind carrier-grade NAT and with a changing address, where a free DNS name or an IP certificate doesn't, and the link already carries a secret over a trusted channel. | The server's TLS mode and the launcher's pinned verifier come out, and hosts use a free DNS name with Caddy. |
| H2 | Over the internet, the write-up's default is open ports; a VPN is the private option (§3.13). | Parent §8 designed open ports, and players install nothing more. The write-up says what an open game port exposes. | The write-up's order changes. |
| H3 | Archives for Linux x86_64 and arm64 (musl, static), Windows x86_64 and macOS arm64, built by a tag workflow, unsigned (§3.7). | M3's D20 targets, plus static Linux builds for any distribution and for ARM servers. | Targets added or dropped in one workflow. |
| H4 | One console service with three front ends: full-screen in its own terminal, plain lines, and attached over a Unix socket on Linux and macOS (§3.1). | systemd gives the server no terminal, and SSH plus `tmux` covers the rest. Windows hosts run in a window. | A Windows named pipe later. |
| H5 | The console adds `invites`, `revoke`, `backup` and `help` to the umbrella's `online`, `accounts`, `maintenance` and `stop`, and its `tp` takes two names (§3.3). | They already exist as subcommands, and a host at the console shouldn't need a second terminal. | Four small commands to drop. |
| H6 | Maintenance persists across restarts, exempts admins and also holds invite redemptions (§3.9). | Hosts turn it on around updates and restarts, and need to check the server before reopening it. A redeemed invite would be spent on an account that can't sign in. | A table and two conditions. |
| H7 | `GET /api/v1/players` lists the names of the characters in the world, to signed-in sessions only (§3.8). | Umbrella §4.1: the approved launcher shows who is online, and only to signed-in players. | One route. |
| H8 | Backups use `VACUUM INTO`, keep 14 copies, and come as a subcommand and a console command (§3.12). | `VACUUM INTO` is consistent while the server writes, and needs nothing beyond SQLite. Parent §8 keeps 14 days. | One function. |
| H9 | A first run with no config at the default path writes one and starts on loopback; an explicit missing path is an error (§3.6). | Easy on a home computer, and safe under systemd. | The first-run branch. |
| H10 | The full-screen console also writes a daily log file, 14 days kept (§3.4). | The pane vanishes with its window, and bug reports need the log. | One writer. |
| H11 | `GET /api/v1/server` carries `source`, from `[server] source_url` (§3.8). | It makes the AGPL's offer to players easy for a host who changes Fenji. | One field. |
| H12 | The write-up is verified in a fresh Debian container with systemd. The real VPS and friends on other machines are M4's acceptance session (§3.13). | Repeatable without the owner's VPS, and close to a fresh server. | A VPS run before v0.5.0, which the acceptance session holds anyway. |
| H13 | Hosting's tasks that change `world/` start after the world plan's Task 16 merges (§2). | Two plans editing the world at once would collide. | Hosting's world tasks wait. |

## 8. Amendment A: as easy as a Minecraft server (the owner, 2026-10-15)

The owner, on the server's settings: "Remember we want hosting to be as flexible and easy as hosting a minecraft server and likewise with people playing". Hosting and joining are measured against a Minecraft server's: one settings file with defaults that work, a start script and a console, an MOTD, ops and bans, and players who type an address and join. Where this section and the earlier ones differ, this section wins. The plan's tasks carry each item.

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| HA1 | **An MOTD.** `[server] motd`, plain text of at most 200 characters, empty by default. `GET /api/v1/server` carries it, and the launcher shows it as the host's notice. A player entering the world gets it as a system line in chat. The console's `motd <text>` and `motd off` change it at once, kept in the database (`server_state`) over the file's. | Minecraft's server list shows each server's MOTD, and the approved launcher already has a place for the host's notice. | A key, a field and two commands. |
| HA2 | **Players play on the address they typed.** With `public_host` unset (now the default), a game ticket's and `GET /api/v1/server`'s `gate_host` is the host the request's `Host` header names, if it's an IPv4 address or a name; otherwise the IPv4 address the request arrived on. `public_host` stays as an override. | In Minecraft the address a player types is the address they play on. A host on a LAN, a VPN or behind port forwarding configures no address at all. | `public_host` back to required. |
| HA3 | **The first run is ready for the LAN.** Once `tls = "self"` exists (Task 4), the first-run template listens on all interfaces (`0.0.0.0:7800`, `0.0.0.0:7801`) with `tls = "self"`. Its first lines say friends on the same network can join now, and how to let in friends over the internet. This supersedes H9's loopback. | A Minecraft server is joinable on the LAN from its first start. Invite-only accounts and the server's own TLS key guard it. | The template's two binds. |
| HA4 | **Invite links for every address.** With neither `public_api` nor `public_host` set, `invite` gives one link for each non-loopback IPv4 address of the machine (LAN and VPN interfaces, through the `if-addrs` crate: no network traffic), each labelled with its interface, and a line on internet play. With either set, one link, as before. | The host doesn't need to know their LAN or VPN address to invite someone. | A loop over interfaces. |
| HA5 | **Ops and bans from the console, live.** `op` and `deop` (kept in the database, alongside `[access].admins`), `ops`, `ban <account> [reason]` (refuses sign-in and game tickets with the reason, and lets the account go at once if online), `pardon`, `bans`. `say` and `list` are aliases of `announce` and `online`. | Minecraft hosts manage players without editing files or restarting. | Two tables and seven commands. |
| HA6 | **Playing (for the launcher's spec, #146).** A server list like Minecraft's multiplayer screen: add a server by address or invite link, and see each one's name, MOTD, players online of the cap, state and ping, then play with one click. Finding LAN servers by themselves is a later option. | The owner's "likewise with people playing". | The launcher's spec decides. |
