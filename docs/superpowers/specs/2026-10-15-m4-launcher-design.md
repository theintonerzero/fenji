# M4 desktop launcher: the server list, the player's copy and Play — Design Spec

**Status:** written on 2026-10-15 under the owner's pre-approval of specs, plans, updates and releases ("I pre approve updates, releases, plans/specs"). Every choice the owner hasn't made is a ruling, L1 to L18 in §7, with what it costs if it's wrong, so the owner can reverse any of them.

**Parent:** the [M4 umbrella spec](2026-10-13-m4-social-sandbox-design.md) §3.3 (the launcher), §4.1 (server info), §4.4 (addresses) and §4.5 (releases); the [hosting spec](2026-10-14-m4-hosting-design.md) §3.8 (server facts), §3.9 (maintenance), §3.10 (TLS without a domain), §3.11 (invite links) and Amendment A's HA6 (the server list). The issue is #146. The approved design is [`DESIGN.md`](../../../DESIGN.md), [`PRODUCT.md`](../../../PRODUCT.md) and the boards in [`docs/design/launcher/`](../../design/launcher/README.md).

## 1. Purpose

The launcher is done when a player who isn't technical goes from their host's invite link to the game's lobby on Linux without help:
- they paste the link and create their account;
- they get their own copy of the game ready: they choose its folder, or download the pinned build from their own Steam library through DepotDownloader; the launcher checks it and repairs what fails;
- they press Play.

They keep a list of servers, as on Minecraft's multiplayer screen (HA6), and see at a glance which are up, how full they are and what their hosts say. The launcher updates itself from signed releases. It looks as the owner approved on 2026-10-13, with one board added for the server list (§3.3).

## 2. Constraints

- **The ground rules hold.** In particular:
  - rule 1: the player's own copy, and nothing from the game ships with the launcher or downloads from Fenji;
  - rule 3: no game art, fonts or logos, only Fenji's own mark;
  - rule 6: M4 starts the unmodified client, and patching comes after M4;
  - rule 7: Fenji's code never reads a Steam or Gameforge login or password (§3.6).
- **Play on Linux only,** through umu-run, as the owner decided for players on gate #114. Windows waits on the AGT decision, and macOS has no way to run the client.
- **No telemetry.** The launcher talks to the servers in its list, and to GitHub for its own updates and the two tools it fetches (§3.6, §3.7). Nothing else. DepotDownloader talks to Steam, and umu-launcher to its own sources.
- **A strict content security policy** (`default-src 'self'`): the fonts, images and scripts ship inside the app.
- **The stack is PRODUCT.md's:** Tauri 2, TypeScript and Vite with no UI framework, over `crates/fenji-launcher`.
- **Licences:**
  - Rust dependencies stay within `deny.toml`'s licences;
  - npm packages are MIT, ISC, BSD, Apache-2.0 or 0BSD;
  - the fonts are OFL-1.1.
- **Order with hosting:** the launcher's tasks that touch `crates/fenji-launcher` start after hosting Task 5 (pins in the launcher) has merged. Its one server change (L9) waits for hosting's tasks on `config.rs` and `api.rs`. The plan orders them.

## 3. Design

### 3.1 A Rust core and a thin view (L1)

- **`crates/fenji-launcher` gains the launcher's model:** the server list, probes, account sessions, the client's checks, the download and repair runner, Play and update state. It owns all state and every secret. It is a library in the Cargo workspace, tested by `cargo test` without a webview.
- **`apps/launcher/src-tauri` is a thin Tauri 2 shell.** It holds the model, exposes one command that takes every intent (the plan's LP1), and pushes a snapshot of the state to the view whenever it changes. It sits outside the Cargo workspace, with its own `[workspace]`, lockfile and CI job, because building a webview needs system libraries that the workspace's other jobs and the cloud agents shouldn't need. The root `Cargo.toml` excludes it.
- **`apps/launcher/src` is the view:** TypeScript modules that render a snapshot into screens and send intents. Its only state is what's on screen: the tab, the form fields and an open dialog.
- **The contract** between them is a `Snapshot` (JSON, from serde) and the intents. Golden snapshots, one for each state a board shows, are written by Rust tests and rendered by the view's tests. A change on either side fails the other's tests.
- **Secrets:**
  - a password crosses from a form to the core once, in its intent, and the core drops it after the request;
  - the session token and the game ticket never reach the view.

What a snapshot holds:
- `servers`: for each saved server, its id, name, address, whether it's pinned, its host's name and its state. The states are online, maintenance, offline, key-changed, own-key and incompatible (a different game build, or a newer API than the launcher knows). Then its players (online, of the cap), MOTD, maintenance message and end time, ping, when it was last seen, and the account (username, signed in or not);
- `active`: the server Play uses;
- `lobby`: who is online on the active server, while signed in;
- `client`: the folder, and its check: none, checking (files done of total), verified (files and build), needs repair (each file and its problem) or missing. Also a download or repair in progress: a percentage and the current file, or waiting for the player to sign in to Steam;
- `game`: idle, starting, running, or ended with a code;
- `platform`: whether this system can play, and if not, why;
- `update`: the current version, an available or downloaded one, and its notes;
- `notice`: the last error, in words (§3.12).

### 3.2 Screens

The header's tabs are **Play · Servers · Account · Client** (the boards' "Server" tab becomes "Servers", L7), then the account pill, Settings, Minimise and Close.

| Board | Shows when |
|---|---|
| Welcome · first run | there is no saved server |
| Account · create from an invite | an invite link was pasted or added |
| Account · sign in | Play or the Account tab needs a session the active server doesn't have |
| Account · signed in | the Account tab, with a session |
| Play · ready | the active server is online and the copy is verified; Play signs in first if needed |
| Play · checking files | a check is running |
| Play · game running | the game is running |
| Play · server in maintenance | the active server is in maintenance |
| Play · server unreachable | the active server doesn't answer, or its key changed or its pin is missing, each with its own words (§3.12) |
| Play · Windows and macOS | the system isn't Linux |
| Servers (new, §3.3) | the Servers tab |
| Client · set up the game | no folder is chosen |
| Client · downloading from Steam | a download is running |
| Client · one file needs repair | the last check found files to repair, however many |
| Settings | the Settings button |
| Launcher update ready | an update has downloaded; once per version |

The boards' words are the launcher's copy. "Moonward", "Wen" and the counts give way to the real values.

### 3.3 Servers (HA6; L7, L9, L10, L18)

- **The list** is the Servers tab: one row per saved server, in the order they were added. A row shows:
  - the server's name;
  - its MOTD on one line;
  - its status line, such as "Online · 3 of 10 · 24 ms", "In maintenance · back around 21:00", "Can't reach · last seen today at 18:42" or "Its key has changed";
  - who hosts it.

  The active server's row is marked. Selecting a row makes that server active. Its row's Play button, the page's one ember action, also makes it active and plays (§3.7), so a player whose copy is ready is one click from the game.
- **Add server** opens a dialog. It takes an invite link or an address:
  - `host` or `host:port`, read as `https://`, port 443 by default, checked by the web's usual authorities (L18);
  - an `https://` address, with or without `#pin=`;
  - `http://` on this machine only (M3 D12).

  An invite link adds its server and opens "Create your account". A server with its own key (hosting §3.10) comes only from a link, since only the link carries its pin.
- **The context column** shows the selected server's facts: the address, its Fenji version, its players, its host, and a link to its source code (the AGPL's §13, from `source`). "Remove from list" is at its foot. Removing a server forgets its address, pin and username. It doesn't touch the account on the server.
- **Probes (L10):**
  - `GET /api/v1/server` runs for the active server every 15 s and when the window gains focus;
  - it runs for every listed server when the Servers tab opens, and every 30 s while it shows; "Refresh" probes at once;
  - the ping is the request's round trip, on a connection kept open between probes;
  - no answer within 5 s makes a server offline. "Last seen" is saved with the server.
- **Who's online:** `GET /api/v1/players` with the session, every 15 s, on the Play and Servers tabs. Without a session, the context column says "Sign in to see who's online". The API gives names only, so the list shows names, and "You" beside the player's own name.
- **Hosted by (L9):** `GET /api/v1/server` gains an optional `host`, the host's name from `[server] host` in the config. It holds at most 32 characters and passes the MOTD's text check (hosting §3.8). The launcher shows it as "hosted by Wen" and as the "Wen:" before the MOTD. Without it, both are left out, and the words that name the host fall back to "your host".
- **Later:** finding servers on the LAN by themselves (HA6).

### 3.4 Accounts (L8)

- **An account belongs to a server.** On Sign in, "Remember this server", ticked by default, keeps the server in the list with the username. Unticked, the server is forgotten when the launcher closes.
- **Redeeming an invite** takes its code (from the link, or typed), a username and the password twice. The server's refusals come back in words: a taken name, an invite that is spent or expired, maintenance holding redemptions (hosting §3.9).
- **Signing in** takes a username and a password. The session lives in memory until it expires, the player signs out, or the launcher quits. Nothing secret is written to disk (M3 D14), so after a restart the player signs in again.
- **Signing out** ends the session on the server too.
- **Refusals at sign-in** use hosting §3.9's answers: a wrong password, a ban with its reason, and maintenance with the host's message. Each is in §3.12's words.

### 3.5 The client (L11)

- **The folder** is chosen in the system's folder dialog. A download's default is `~/Games/SoLO`.
- **Checks** run against the pinned manifest the launcher ships, which holds hashes only (M3):
  - **quick**, every file's size and the hashes of `bin64`'s executables (M3 D15), when the launcher starts, after a folder is chosen, and before Play;
  - **full**, every file's SHA-256, after a download or a repair, and when the player asks ("Check every file").

  A check runs off the interface's thread, shows its progress on the blade, and can be stopped.
- **The results** go file by file: fine, missing, wrong size, or doesn't match the build. Files the manifest doesn't list are left alone.
- **Repair.** On Linux, "Repair from Steam" has DepotDownloader fetch only the failing files (§3.6), then fully checks those files again, and gives every file the quick check. The record then says "Quick check", since the other files weren't hashed again (#348). Under it, the board's instruction tells a Gameforge owner which files to copy over again from that install. The launcher can't tell the two kinds of copy apart, so it offers both.

### 3.6 Downloading with Steam (L2, L3, L4)

- **Linux only** in M4 (L2).
- **The tool** is DepotDownloader 3.4.0 for Linux x64. It is fetched on first use from its GitHub release, checked against a SHA-256 in the launcher's source, and kept in the launcher's data folder. It is never bundled.
- **Signing in to Steam** happens in DepotDownloader's own terminal window.
  - The launcher opens the system's terminal: `xdg-terminal-exec`, then `$TERMINAL`, then a short list of common terminals.
  - The terminal runs a small script Fenji ships, which starts DepotDownloader with `-qr`. The player scans the code with the Steam app, as on Steam's own sign-in page.
  - No Steam login or password is typed into Fenji or into any script of Fenji's (ground rule 7, L3). DepotDownloader 3.4.0 takes a login only on its command line, so the QR code is the one sign-in that keeps the login out of Fenji's code.
- **Without the Steam app,** or when no terminal can be found, the screen shows the exact command to run in the player's own terminal, on one line: their folder and the pinned build filled in, their own login left for them to type. Then they choose the folder. The command runs the DepotDownloader Fenji fetched, if it has one. Otherwise the dialog says where to get the pinned release, and the command makes it executable and runs it from its folder. Repair has a command of its own: the failing files, which Fenji lists in a file of its own, with `-validate`, then a check of the files (#348).
- **The pinned build only:** app 1418100, depot 1418101, manifest 5002037851744157048, with `-os windows -osarch 64`, into the chosen folder. A repair adds `-filelist` with the failing files.
- **Progress:**
  - the script shows everything DepotDownloader prints in the terminal;
  - it passes the launcher only the progress lines (a percentage and a file path) and the exit status, through files in the launcher's data folder;
  - the blade climbs with the percentage.
- **No `-remember-password`,** so no Steam login token is kept, and a later repair signs in again.
- **Cancel download** ends DepotDownloader. The files fetched so far stay, and a later download skips the ones that already match.
- **The end.** When DepotDownloader exits with 0, a full check runs. Otherwise the screen says the download stopped, with its status, and offers to try again.

### 3.7 Play (L2, L4, L12)

- **On Linux, Play:**
  1. signs in if it has to (the Sign in page, then back);
  2. runs the quick check;
  3. makes the Wine prefix ready;
  4. takes a single-use ticket with the session, which lives 60 s, so first-start setup never spends it;
  5. passes the login path's environment to the client (`gj_uinfo`, `gj_gates` and `gj_ticket`; gate #114);
  6. starts the unmodified client with `umu-run` in the launcher's own prefix.

  These are `fenji-launcher`'s `start` steps, with the session the model holds, where `fenji-launch` signs in for each game.
- **umu-run** is the one on `PATH`, or umu-launcher 1.4.4's zipapp, fetched on first Play from its GitHub release and checked against a SHA-256 (L4).
  - The first start makes the prefix, and umu-run downloads Proton and Valve's runtime. That takes a few minutes, so Play says "Starting · the first start sets up Proton, which takes a few minutes".
  - **Cancel start,** beside Play while it starts, ends the start before the game runs: umu-run making the prefix, with everything it started, or the ticket (#348). Making the prefix may take 60 minutes before Fenji stops it. A prefix whose making was stopped is made again on the next start.
  - `PROTONPATH` in the launcher's environment, or the umu-run path in Settings, overrides the defaults, as `fenji-launch`'s options do.
- **While the game runs:**
  - Play reads "Running", and the lobby lists the player;
  - one game runs at a time;
  - closing the launcher leaves the game running.

  When the game ends, Play opens again. A non-zero exit code is shown, with "Open the log folder". So is an end within 15 s of the start, whatever its code: umu-run 1.4 ends with 0 when it fails (#332).
- **On Windows and macOS,** Play stays disabled with "Fenji can't start the client on this system yet" and the list of what still works there (the Unsupported board).

### 3.8 Updates (L5, L6)

- **The check.** With "Update automatically" on, the default, Tauri's updater checks `https://github.com/theintonerzero/fenji/releases/latest/download/latest.json` when the launcher starts.
  - A newer version downloads in the background, and its signature is checked with Fenji's release key, whose public half is built into the launcher.
  - The context column's foot then reads "Launcher 0.5.0 · 0.5.1 ready", and the update dialog shows once, with "Later" and "Restart and update", the one ember button.
  - Offline, the check fails quietly.
- With "Update automatically" off, nothing is checked until the player presses "Check now" in Settings.
- **What a release builds:** the Linux AppImage (x86_64), the Windows NSIS installer (x86_64, per user) and the macOS app (arm64), each with its updater signature, and `latest.json`. No other Linux package is built, since the updater can't update one.
- **No OS vendor signs them:** no Authenticode, no notarization (L6). The release notes say how to open the launcher the first time on Windows and macOS.
- **The key (L5)** was made with Tauri's signer on the owner's game box on 2026-10-15, when the plan needed its public half (key ID 02D8621117AE26ED).
  - Its private half stays outside every repository, in `~/.config/fenji/release-key/` (owner-only), and in the repository's Actions secrets.
  - Its public half is committed in `tauri.conf.json`.

### 3.9 The window and the shell (L13)

- **The window** is 1200 × 720: not resizable, no maximise. It's undecorated, with the header as the drag region and its own Minimise and Close.
- **One instance:** starting the launcher again brings the running one forward.
- **What the view may do.** The webview loads only the app's own files. Tauri's capabilities give the view only the launcher's command, its snapshot event and dragging the window. The shell does the rest when the view asks through an intent (the plan's LP10):
  - the window's Minimise and Close;
  - the folder dialog;
  - opening links in the system browser, for the repository's pages and each server's `source` address only.
- **English only.**
- **No link handler:** invite links are pasted (hosting §3.10).

### 3.10 The look (L14, L15)

- **The tokens:** DESIGN.md's colours, type, spacing, radii and shadows become CSS custom properties.
- **The fonts:** Cinzel, Geist, Geist Mono, and Noto Serif SC subset to 焚寂, bundled as WOFF2 with their OFL licences.
- **The blade** is inline SVG in its four states (DESIGN.md), with `role="progressbar"` and its readout wherever it shows progress.
- **The three gaps the boards left are fixed:**
  - input borders reach 3:1 against their ground;
  - the vignette follows the server's state on every board (ClientSetup and Unsupported showed a cool vignette with the server online);
  - the update dialog has one ember button.
- **The Servers board** is designed in the same language with the `impeccable` skill before it's built, and recorded in DESIGN.md.
- **The build ends** with impeccable's finish review against the boards, and DESIGN.md updated from the shipped launcher.
- **Into the repository with this spec:**
  - the approved boards, under `docs/design/launcher/`;
  - `DESIGN.md` and `PRODUCT.md`, at the root;
  - `.impeccable/design.json` and the surface brief, `.impeccable/surfaces/apps-launcher.md`, where the `impeccable` skill reads them.

### 3.11 Data on disk and privacy (L16)

- **The config folder** holds `settings.json`: the servers (address, pin, username, name, last seen), the active one, the client folder, "Update automatically", and an umu-run path if one is set. M3's file, which holds one server, becomes the list's first entry when it is first loaded.
- **The data folder** holds:
  - the Wine prefix and the fetched tools;
  - DepotDownloader's progress and status files;
  - the launcher's log, rotated, which never holds a password, token, ticket or Steam detail.
- **PRIVACY.md gains:**
  - the update check to GitHub, and that "Update automatically" turns it off;
  - the one-time downloads of DepotDownloader and umu-launcher from GitHub;
  - that umu-launcher downloads Proton and Valve's runtime on the first start;
  - the log.

### 3.12 Words for what goes wrong

Every error is a sentence in DESIGN.md's voice: what happened, and what to try.

| Cause | Words |
|---|---|
| No answer | "Can't reach Moonward · last seen today at 18:42", and the Offline board's list of what to try |
| A pin that doesn't match | "Moonward's key has changed. Ask Wen for a new link." |
| A server's own key, with no pin | "Moonward uses its own key. Open the link Wen sent you." |
| A wrong username or password | "That username and password don't match an account on Moonward." |
| A ban | "Wen has banned this account from Moonward: \<reason\>." |
| Maintenance at sign-in | "Moonward is in maintenance: \<message\>." |
| A taken name | "“lin” is already taken on Moonward. Try another name." |
| A spent or expired invite | "That invite has been used or has expired. Ask Wen for a new one." |
| Another game build | "Moonward expects a different build of the game than this launcher checks." |
| A newer server | "Moonward runs a newer Fenji. Update the launcher." |
| umu-run failing | "The game couldn't start: umu-run ended with \<code\>. The log folder has the details." |
| umu-run ending with 0 within 15 s (#332) | "The game couldn't start: umu-run ended at once. The log folder has the details." |
| DepotDownloader failing | "The download stopped: DepotDownloader ended with \<code\>. Try again." |
| No terminal | "Fenji couldn't open a terminal for DepotDownloader. Open one yourself, and run the command under "No Steam app? Run DepotDownloader yourself" on the Client tab." (#348) |

Without a host's name, "Wen" becomes "your host".

## 4. Testing

- **The model** (Rust, in the workspace):
  - the server list, and its migration from M3's settings;
  - the Add dialog's address parsing;
  - each probe state against an in-process server: online, maintenance, offline, key changed, own key, incompatible;
  - nothing secret on disk: after a whole flow, a test searches the settings file and the log for the password, the token and the ticket;
  - a check's progress and stop, and the repair's file list;
  - the DepotDownloader runner against a fake DepotDownloader (progress lines, exit status, cancel), and the terminal chosen from a fake environment;
  - Play's states with a fake client process;
  - the golden snapshots.
- **The view** (TypeScript, vitest with jsdom) renders each golden snapshot and checks:
  - the words;
  - one ember button at most;
  - the disabled states and their reasons;
  - the focus order and ARIA (progressbar, dialog, status).
- **The shell:** clippy and a build in CI's launcher job. The release workflow's dry run builds the three bundles and `latest.json`. A local check installs one test version's signed update over another from a local server.
- **The look:** the finish review compares screenshots of each state, rendered from the golden snapshots, with the boards.
- **End to end,** on the game box under the bare-metal guards (CLAUDE.md, "Bare-metal client runs"): the AppImage takes a new player from an invite link to the lobby, against a local `fenji-server` with `tls = "self"`. The player redeems, chooses the pinned copy, runs a full check, and plays. `scripts/spike/run-bare.sh` gains a way to start the desktop launcher in place of `fenji-launch`.

## 5. Done when

- **End to end:** on Linux, a new player goes from an invite link to the lobby in the AppImage, in §4's local run.
- **Servers:** the tab lists servers with their name, MOTD, players, state and ping. A player adds one by address or by link, removes one, and plays on one with one click.
- **States:** maintenance, an unreachable server, a changed key and a missing pin each show their words, and Play stays closed with its reason.
- **Checks and repairs:** quick and full checks report file by file. A repair fetches only the failing files, which the fake DepotDownloader verifies. A real Steam download needs the owner's Steam sign-in, so it is the owner's check.
- **Releases:** the release workflow builds the AppImage, the Windows installer and the macOS app, signed for updates, with `latest.json`. A local update from one test version to the next installs.
- **Other systems:** on Windows and macOS the launcher builds, and Play stays disabled with its reason. CI builds them; nothing runs them here.
- **The look:** the finish review's verdict is to ship, and DESIGN.md describes the shipped launcher.
- **The docs:** PRIVACY.md, README.md and docs/README.md say what the launcher does, and `fenji-launch` still works.

## 6. Out of scope

- Finding LAN servers by themselves, a link handler for invite links, and languages other than English.
- Play on Windows (the AGT decision) and on macOS, and Steam downloads there.
- Keeping passwords or sessions in the system's keychain.
- Signatures from OS vendors.
- News, a store, or anything else that isn't about getting to Play.
- Client patches (after M4; ground rule 6).

## 7. Rulings

Each is the controller's, made under the owner's pre-approval, and recorded so the owner can reverse it.

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| L1 | The Rust core owns the state and every secret, and the TypeScript view only renders snapshots. The Tauri shell sits outside the Cargo workspace, and golden snapshots are the contract between them (§3.1). | The logic is tested without a webview, secrets never reach the webview, and the workspace's gate and the cloud agents need no webview libraries. | The model's code moves into the view, behind the same intents. |
| L2 | Windows and macOS builds ship with Play disabled. Steam download and repair are Linux-only in M4 (§3.6, §3.7). | The approved Unsupported board, and nobody should download a copy that Fenji can't start on their system. | Two platform checks to lift once Windows play is decided. |
| L3 | Steam sign-in is by QR code in DepotDownloader's own terminal window. Without the Steam app, the player runs the command shown, with their own login, in their own terminal. There's no `-remember-password` (§3.6). | DepotDownloader 3.4.0 takes a login only on its command line, so anything that typed it there for the player would be Fenji's code reading a Steam login (ground rule 7). | A login prompt in the terminal script, if the owner rules a login name isn't a credential. |
| L4 | DepotDownloader and umu-launcher are fetched on first use from their GitHub releases, at pinned versions with SHA-256 checks, and never bundled. umu-run on `PATH` wins (§3.6, §3.7). | The installers carry no other project's programs (DepotDownloader is GPL-2.0, umu-launcher GPL-3.0) or their notices, and moving a pin is a launcher update. | Bundle them, with their licences, in the release workflow. |
| L5 | Fenji's update key is made by the controller on the owner's game box, under the 2026-10-15 pre-approval. Its private half stays outside every repository and in Actions secrets (§3.8). | Self-update is in the owner's brief, and the owner left the key to the release. | Before v0.5.0 ships: free, by replacing the key. After: one update signed with the old key moves launchers to the new one. |
| L6 | The bundles are an AppImage (x86_64), a per-user NSIS installer (x86_64) and a macOS app (arm64). No OS vendor signs them (§3.8). | The updater updates exactly these. Vendor signing costs money and needs the owner's accounts. | Targets added in one workflow, and signing when the owner has the accounts. |
| L7 | The boards' Server tab becomes Servers: HA6's list. One active server drives Play, and a row's Play plays on it (§3.3). | The owner's Minecraft bar, with the approved Play screens unchanged. | One tab's design. |
| L8 | Accounts are per server. Nothing secret goes to disk, so a player signs in once per launcher run (§3.4). | M3's D14, and no keychain dependency on three systems. | Keychain storage behind "Remember me". |
| L9 | `GET /api/v1/server` gains an optional `host`, from `[server] host` (§3.3). | The approved boards name the host everywhere ("hosted by Wen"), and no field carried it. | One config key and one JSON field, both optional. |
| L10 | Probes: the active server every 15 s, the list every 30 s while it shows, a 5 s timeout, and the ping from a kept connection (§3.3). | Fresh enough to see a server come back, at little load for a 20-player server. | Numbers in one place. |
| L11 | A quick check at start, after choosing a folder and before Play. A full check after a download or repair, and when asked (§3.5). | Sizes catch a missing or half-written copy at once. A full check of every file takes minutes. | When each check runs. |
| L12 | The launcher stays open while the game runs. Closing it leaves the game running, and one game runs at a time (§3.7). | The lobby list and Running need the launcher, and quitting it shouldn't end a session. | The window's behaviour while the game runs. |
| L13 | A fixed, undecorated window, one instance, Tauri capabilities limited to what the view needs, and no link handler (§3.9). | DESIGN.md's window, and the least the webview can do. | A link handler later (hosting §3.10). |
| L14 | DESIGN.md's tokens as CSS custom properties, the fonts as bundled WOFF2 under the OFL, and the blade as inline SVG (§3.10). | The CSP forbids remote fonts, and the blade animates its fill and clip. | Asset formats. |
| L15 | The boards and design files enter the repository with this spec. The Servers board is designed with `impeccable` before it's built, and the boards' three gaps are fixed (§3.10). | Cloud implementers need the boards, which lived only in a session's scratch folder. | Files to move. |
| L16 | PRIVACY.md lists the update check, the two tool downloads and umu-launcher's own downloads. "Update automatically" turns the check off (§3.11). | PRIVACY.md today says Fenji makes no update checks. | Wording. |
| L17 | A download goes to `~/Games/SoLO` unless the player chooses another folder (§3.5). | The approved ClientSetup board. | A default path. |
| L18 | In Add server, a bare host means `https://` on port 443, checked by the usual authorities. Servers with their own key come by link (§3.3). | Only a link carries a pin, and plain http stays on this machine (M3 D12). | One default. |

## 8. Amendment A: Steam copies, and DepotDownloader inside the launcher (the owner, 2026-10-16)

The owner, after v0.5.0: "depot downloader for those who dont have access to the game via steam app right, if downloaded via steam can use that copy etc (and the qr and hidden depot downloader within the launcher too)". A player whose copy Steam installed uses it as it is. A player who owns the game on Steam but hasn't got it signs in to DepotDownloader inside the launcher, by QR code. Only a password sign-in, for a player without the Steam Mobile App, still opens a terminal. Where this section and the earlier ones differ, this section wins. The follow-ups plan's tasks carry each item, built inline as the launcher was.

- **A copy Steam installed (LA1).**
  - Steam's own files list the player's libraries: `libraryfolders.vdf` under Steam's folder. On Linux that folder is `~/.steam/steam`, `~/.local/share/Steam`, or the Flatpak's or Snap's. On Windows, it's the folder the registry names, or the default.
  - A library with `steamapps/appmanifest_1418100.acf` holds the game, in `steamapps/common/` under the manifest's `installdir`. The manifest's `buildid` tells at once whether it's the pinned build, 10015727.
  - Found, the Client tab's setup screen offers it first: "Found in your Steam library", the folder, and "Use it", the screen's one ember. Using it is choosing that folder, so the quick check runs as for any folder. A different build says so, and asks the player to update the game in Steam.
- **Fenji never writes into a Steam library (LA2).** A copy there that needs repair is repaired by Steam itself. The Repair screen names Steam's "Verify integrity of game files", in place of "Repair from Steam". DepotDownloader stays for copies in a folder of the player's own.
- **Play never goes through Steam (LA3).** The launcher starts a copy from a Steam library as it starts any other: through umu-run on Linux, so it can hand the game its ticket and keep one game at a time (§3.7). Steam may be running. If the acceptance session shows that the unmodified client, started this way, hands itself to Steam or stalls, the launcher asks the player to close Steam before Play. After M4, the patcher can remove the client's Steam and Gameforge SDK calls (ground rule 6). A patched client started *through* Steam would reach Steam: that is a decision gate, and nothing here does it.
- **DepotDownloader runs hidden, and the QR code shows in the launcher (LA4).**
  - Download and Repair run DepotDownloader 3.4.0 as the launcher's own child, with `-qr`. Its input is closed and its output goes to the model.
  - It draws the sign-in code as text, two characters a module (QRCoder's `AsciiQRCode`): after "Use the Steam Mobile App to sign in with this QR code:", and again after "The QR code has changed:".
  - The model reads those lines as the code's squares, and the Downloading screen draws them, with "Scan this with the Steam Mobile App". Fenji never decodes the code: it redraws what DepotDownloader drew.
  - Progress lines are read as in §3.6, and the run's end is the child's exit. Cancel ends the child's process group.
  - Every other line is dropped unread: never logged, shown or kept. That includes the account name DepotDownloader prints after a sign-in (ground rule 7).
  - The code is a sign-in request that only the player's Steam app can approve, and the session goes to DepotDownloader. No password or token passes through Fenji. There's still no `-remember-password`, so each download signs in again.
  - DepotDownloader saves each sign-in itself, whatever its flags: the account name and Steam's token, in its own settings (`account.config`, in .NET's isolated storage under `$XDG_DATA_HOME`; 3.4.0's `Steam3Session.cs`). Fenji never has it use them again, so each hidden run gets a data folder of its own in its run folder, as its `XDG_DATA_HOME`. That folder goes as the run ends: finished, failed, cancelled, or stopped as the launcher exits; after a crash, the next launcher removes it (#443). The password route's command runs where the player runs it, and DepotDownloader keeps its file in its usual place there, as PRIVACY.md says.
- **A password sign-in is the player's own command (LA5).** "No Steam Mobile App? Sign in with your password" ends the hidden run and shows the command for the player's own terminal, with their own login, as L3 described for a player without the Steam app (#348). The password is typed into DepotDownloader there, never into Fenji. (The final review of #429 ruled so: L3's terminal is QR-only, and only the player may type a login, ground rule 7, so a terminal Fenji opened could never take a password. That terminal went with it.)
- **Steam's Proton (LA6).** On Linux, when a Steam library holds Proton, umu-run uses the newest stable `Proton x.y` there (`PROTONPATH`), unless the player set one. So a player with Steam skips Proton's download on the first start. This comes after a game-box rehearsal shows that version starting the client through umu-run.

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| L19 | A Steam install is found through Steam's library files, and offered first on the setup screen. Fenji never writes into a Steam library, and Steam's "Verify integrity" repairs a copy there (LA1, LA2). | The player has the copy already, and Steam's record of it must stay true. | A repair in place later. |
| L20 | Play starts the game itself, never through Steam (LA3). | The ticket, one game at a time, and ground rule 6's gate for a patched client. | Closing Steam before Play, should the acceptance session need it. |
| L21 | DepotDownloader runs hidden with `-qr`, and the launcher redraws its QR code from DepotDownloader's own output. A password sign-in is the player's own command in their own terminal. This replaces L3's terminal (LA4, LA5). | The owner's decision. No credential passes through Fenji, and the window that opened beside the launcher goes. | The terminal path back for everyone, from git (15a4cda). |
| L22 | umu-run uses Steam's newest stable Proton when a library holds one, after a rehearsal (LA6). | The first start's largest download, skipped. | umu's own Proton, as before. |
| L26 | A hidden run's `XDG_DATA_HOME` is a folder of its own, removed as the run ends (LA4; #443). | DepotDownloader saves a Steam token after every sign-in, which Fenji never uses again: on the player's disk it's only a risk. | The token where DepotDownloader keeps it, as before #443. |

## 9. Amendment B: Play on Linux, tuned (the owner, 2026-10-17)

The owner, on 2026-10-17, after Amendment A's tasks were built: the latest GE-Proton always ("proton ge latest always!"), and GameMode and a launch options field ("we need as much QOL as possible!"). Where this section and the earlier ones differ, this section wins. The follow-ups plan's tasks carry each item, built inline as the launcher was.

- **The latest GE-Proton (LB1).** On Linux, unless the player set `PROTONPATH`, Play starts the game with the latest GE-Proton (GloriousEggroll's `proton-ge-custom`), the Proton family Fenji's client runs use (pinned there, the latest here). Since 10-10, GE-Proton uses NTSYNC where the kernel has it.
  - The launcher fetches it itself, as it fetches DepotDownloader and umu-launcher: from GE-Proton's latest GitHub release, checked against that release's own SHA-512 sum, into its data folder's `proton/`, and passes that folder to umu-run as `PROTONPATH`. A newer release replaces the one it has, which is removed once the new one is in place.
  - It asks GitHub for the latest release before each start. PLAY shows the fetch as it comes ("Getting GE-Proton 42%"), as it shows umu-launcher's. Offline, or when GitHub doesn't answer, the build it has plays; with none yet, umu-run's own default, as before.
  - The Wine prefix remembers the Proton that set it up. A start with another Proton sets it up again before the ticket is taken, as a first start does, so Proton's upgrade of the prefix never ages the ticket inside the game's own run (the final review of this work, I4).
  - A player's own `PROTONPATH`, from the launcher's environment or the launch options (LB3), reaches umu-run as it is, never anchored to the launcher's folder: umu-run's own names (`GE-Proton`, `UMU-Latest`, a folder in Steam's `compatibilitytools.d`) work as in umu-run (I2).
  - This replaces LA6 (Steam's Proton): every player on one runtime, the one the client runs test.
- **GameMode (LB2).** When Feral GameMode's `gamemoderun` is on the `PATH`, Play starts the game under it: the CPU's performance governor, and the game's scheduling and I/O priority, while it runs. Settings has a GameMode switch, on by default, shown only when GameMode is installed and the launcher's environment reaches a D-Bus session bus, as libdbus finds one (an address in `DBUS_SESSION_BUS_ADDRESS` that doesn't autolaunch, or the user's own socket at `$XDG_RUNTIME_DIR/bus`). GameMode's daemon is reached over that bus. With none, libdbus autolaunches `dbus-launch`, which `gamemoderun`'s preload makes ask again, without end, and the game never starts (Task 11's rehearsal, where `run-bare.sh` hides the bus: 51,262 processes in six minutes). There, the game starts without GameMode.
- **Launch options (LB3).** Settings has a Launch options field, as Steam's: `NAME=value … wrapper … %command% arguments…`.
  - The text is split into words as a POSIX shell splits them (quotes and backslashes), and never run through a shell.
  - Words of the form `NAME=value` before any other word set the start's environment: `PROTONPATH` among them, which is how a player sets their own Proton. The words before `%command%` wrap the start, outermost first (`mangohud`, `gamescope -f --`); the words after it are added to the game's arguments, after Fenji's own. Without `%command%`, the words are the game's added arguments, as in Steam.
  - The start's order: the player's wrappers, then `gamemoderun`, then `umu-run`.
  - The variables Fenji sets for the start can't be set there: `WINEPREFIX`, `GAMEID` and the login path's. A field that tries, an unclosed quote, or `%command%` twice is refused in words, and the field keeps the player's text.
  - Nor can the game's own platform flags, anywhere in the field: `--gf-sdk` and the others its library reads (spike 0, 5.10), bare or with a value, and `AUTH_TYPE`, which choose its sign-in platform. Fenji starts the game without them (D1), and gate #11 keeps `--gf-sdk=mocked` from players (the final review, I3).
  - `PROTONPATH` reaches umu-run as typed, with no shell to expand it: a full path from `/`, or a name umu-run looks up itself. One starting with `~`, holding a `$`, or relative is refused in words (I2).
  - Linux only, as Play is. Settings shows the field there only.

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| L23 | The latest GE-Proton by default, fetched and checked by the launcher into its own folder (LB1). | The owner's decision. One runtime for every player, the client runs' own; its progress on PLAY, and its old builds removed. umu-run's own codenames do neither: `GE-Proton` installs each release into Steam's `compatibilitytools.d` and keeps them all, unseen during Starting, and `GE-Latest` updates only through umu's delta patches, which need a module umu may not have, so it can stay at its first build. | umu-run's `PROTONPATH=GE-Proton`, its downloads unseen. |
| L24 | GameMode when it's installed, on by default, with a switch to turn it off (LB2). | The owner's decision: as much quality of life as possible, at no cost where GameMode isn't installed. | GameMode off by default. |
| L25 | Launch options as Steam's, split as a shell would but never run through one (LB3). | Linux players know Steam's form; a shell would run the field's text as code, quotes and all. | A shell's corner cases (globs, `$VAR`) the field doesn't do. |
