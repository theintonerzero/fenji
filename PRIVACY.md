# Privacy

This page says what personal data Fenji's software handles and where it goes. It isn't a contract. The project itself runs no service: every Fenji server is run privately by someone who hosts it for the players they invite (the **operator**), and the operator is responsible for the data on that server.

Fenji isn't a game yet. From v0.5.0 a Fenji server runs accounts, the character lobby and a shared world, where players see each other, move and chat, and the desktop launcher and the command-line launcher start the client on Linux. Where this page describes what isn't built yet, it follows the design ([`docs/superpowers/specs/2026-10-11-fenji-design.md`](docs/superpowers/specs/2026-10-11-fenji-design.md) §5–§8). This page changes as Fenji does; its git history shows how.

## No telemetry

Fenji's own software sends nothing about you to the project or to anyone else. It has no analytics or crash reporters. It talks to the servers you point it at, to Steam through DepotDownloader if you download your game client that way, and, from the desktop launcher, to GitHub:
- **Its updates.** When it starts, the launcher asks GitHub for the latest release's `latest.json`, to see whether there's a newer launcher, and it asks again when you press "Check now" in Settings. With "Update automatically" off, it asks only when you press that. A newer launcher downloads from the same release, and installs only if Fenji's release key signed it.
- **Two tools, once, on Linux.** The first time it needs them, the launcher downloads DepotDownloader and umu-launcher from their GitHub releases, at pinned versions that it checks by SHA-256. A `umu-run` on your `PATH` is used instead of a download.
- **GE-Proton, before each start on Linux.** Before it starts the game, the launcher asks GitHub for GE-Proton's latest release. When that release is newer than the build it has, it downloads it, checks it against the release's own SHA-512 sum, and removes the older build. A `PROTONPATH` you set yourself, in Settings' launch options or the launcher's environment, is used instead, and then it asks nothing.

A server's update script talks to GitHub too, when its host runs it: `update-server.sh`, or `fenji-update` on a VPS, asks where Fenji's latest release is, and downloads that release's server archive and `SHA256SUMS`. The server itself never does: it has no update check.

These requests carry nothing about you or your accounts, though like any request they show GitHub your IP address. The game client is another matter (below).

**Finding a copy Steam installed reads only your computer.** To offer a copy of the game Steam already installed, the launcher reads Steam's list of your libraries (`libraryfolders.vdf`) and the game's install record (`appmanifest_1418100.acf`), on Linux and Windows. On Windows it also reads where Steam is installed from the registry (`SteamPath`). It reads nothing else of Steam's, sends nothing, and never writes into a Steam library.

**DepotDownloader's output stays on your computer.** When you download or repair your game client through Steam, the launcher runs DepotDownloader in the background and reads, of what it prints, only three things: the sign-in code it draws, its progress, and its end. The code is drawn again in the launcher, never decoded, kept or logged; every other line, your Steam account name among them, is dropped unread. A password sign-in is the command the launcher shows, which you run in your own terminal: DepotDownloader asks for your password there, and the launcher reads nothing of it.

**DepotDownloader's own record of your sign-in goes with its run.** Each time you sign in, by code or by password, DepotDownloader saves your Steam account name and the token Steam gave it for that sign-in in its own settings file, `account.config`, whatever it's told; on Linux it keeps that file under `~/.local/share/IsolatedStorage`. The launcher never has DepotDownloader use them again. So for each download or repair it runs in the background, it gives DepotDownloader a folder of its own for that file, in the launcher's data folder, and deletes the folder as soon as DepotDownloader ends, or the next time the launcher starts if it crashed first. A password sign-in runs in your own terminal, where DepotDownloader keeps the file in its usual place. An earlier launcher's downloads, or any you ran yourself, may have left one there too. Deleting it is safe: DepotDownloader just asks you to sign in again.

**A server's region is its host's choice.** `fenji-server init`, or a first run, guesses the region from the computer's own timezone, through the time zone files on that computer, and asks no one. A host who keeps it, or sets one, publishes it with the server's name and its other facts, to anyone who can reach the server's account API, and launchers show it to players.

**The launcher's log stays on your computer.** It notes what the launcher did, such as the servers you added and the account names you signed in with, but never a password, token or ticket (`launcher.<date>.log`, kept seven days). It also keeps what the game printed the last time it started (`game.log`). Both are in the launcher's data folder (`~/.local/share/fenji-launcher/logs` on Linux), which Settings opens with its Logs link. Nothing sends them anywhere.

On Linux the launcher starts the client through [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher), which has network habits of its own:
- it checks for a network route, with a UDP `connect` to `1.1.1.1` that sends nothing;
- it downloads its Steam Linux Runtime from Valve's servers (`repo.steampowered.com`) on first use, and asks them later whether it has an update;
- when it isn't given a Proton build, it checks GitHub for the latest UMU-Proton each time it starts, and downloads it when it's new. The desktop launcher gives it GE-Proton once it has a build, or your own `PROTONPATH`; `fenji-launch` gives it the build you name with `--proton` (or `$PROTONPATH`).

Those requests come from umu, not from Fenji. They carry nothing about your account, though like any request they show your IP address.

## What a Fenji server stores

- **Your account:** your username, your password as an argon2id hash (never the password itself), when the account was created and when you last signed in.
- **Invites:** the one-time codes the operator issues, each with an optional label the operator writes, such as who it's for.
- **Your characters:** for now their name, class, body type, level and appearance (voice, face, outfit and portrait), when they were created, and where they last stood in the game world (a scene, a position and a facing), so they survive a restart. Stats, inventory and skills join them in later milestones.
- **Tokens, as hashes only:** invite codes, session tokens and game tokens are stored as SHA-256 hashes with their times, never as the codes or tokens themselves.
- **Ops and bans:** whether the operator has made your account an op (one of the server's admins), and, if they have banned it, the reason they gave and when.
- **Backups:** the reference setup ([`docs/hosting.md`](docs/hosting.md)) backs up the server's database each night and keeps the last 14 copies; each operator can choose otherwise.

It also handles short-lived tokens: a session token after you sign in, and a single-use game token, valid for 60 seconds by default, when you press Play.

In the game world it passes each chat line on to the players it is for and keeps no copy. Its logs say that a line came or went, and on which channel, but not what it said or whom a whisper was for. The exception is an admin's command (`!tp`, `!kick`, `!announce` or `!invite`): the server logs each one with the admin's account and the command's arguments, so the operator can see what their admins did.

Players who are signed in to the server's launcher see the names of the characters in the world.

Like any server, it sees the IP address you connect from, and its logs may record it. A server with its full-screen console also writes its log to files in its data folder, one for each day it runs, and keeps those of the last 14 days it ran; under systemd, the system's journal keeps the log. The operator decides how long logs and backups are kept.

## The game connection

The game client's connection to a Fenji server is readable and unauthenticated. Anyone on the network path between you and the server can read what crosses it, and could pose as the server. That is how the game client was built, and only a change to the client could fix it. Fenji runs your copy of the client unmodified today. A later version may patch it, only ever so that it talks to Fenji alone, and such a patch could close this gap. Until then:
- **No password crosses it.** Your password goes only to the server's account API; the game connection carries a ticket that works once, for one account, and expires after 60 seconds by default.
- **Someone on that path could still take over your session.** If that matters to you, play over a VPN with your friends, or on a network you trust.

## Trusting a server's host

Joining a Fenji server means trusting the person who runs it. The game client obeys the server it's connected to, and some of the server's messages can write files on your computer, open web pages, replace the client's scripts or run code. Fenji's server never sends them: a list in its code refuses each one ([`lobby.md`](docs/protocol/packets/lobby.md) §2). But a server someone has changed could send them, and the client can't tell the difference. Only play on servers run by people you trust.

## What Fenji never handles

- **Your Steam or Gameforge password.** You type those only into the services' own pages, or into DepotDownloader, which talks to Steam directly. Signing in by code, you scan it with the Steam Mobile App, which approves the sign-in with Steam: the launcher only draws the code DepotDownloader made.
- **Payment details.** Fenji's software has no payment features.

## What the game client sends

When the game client logs in, its own login message includes, besides your account name and a ticket, its version, its language, a network hardware (MAC) address and a 20-byte machine fingerprint. In our test runs, up to the character screens, the client that Fenji's launcher started made one connection, to the Fenji server, and looked no names up. It also tried to read a local file for a server list, which wasn't there; that uses no network. That is the client's behaviour, not something Fenji adds. A Fenji server doesn't need the address or the fingerprint, and the design doesn't store them. Once a character enters the game world, the client also reports, when the world has loaded, its Windows version and whether the program that started it is named `Client.exe`, and sends, with each of its time checks (about one a second), two counters about its own machine. A Fenji server reads none of these, keeps none of them, and its logs show them masked. In the world the client also sends messages its own scripts call logs, among them the name of the program that started it, its frame rate and how its scenes loaded: a Fenji server ignores them, and its logs name only which message arrived. On entering the world, the client starts a helper of its own for web pages (`browserhost.exe`); in our first test run it made no connection.

The game client is the game developer's software, not Fenji's, and Fenji can't speak for everything it does. It ships crash-reporting components (`AurCrashRpt64.dll`, `sentry.dll`, `crashpad_handler.exe`) that may upload a report when it crashes, and it loads the Steam and Gameforge platform libraries. In the sandboxed test runs where we checked, the crash helpers didn't start; a player's own sessions haven't been examined.

Developers who study the client run it inside a sandbox, against their own stub or server; play tests outside the sandbox happen only on the project's own Linux machine, with the client's network limited to a local Fenji server. Those runs' logs stay on the developer's machine, in a folder git ignores. Test fixtures in this repository, which come from such runs, have the address and fingerprint replaced with made-up values, and a test checks that they are.

## Deleting your data

Ask your server's operator. There is no self-service deletion yet. The operator runs `fenji-server account delete <username>`, and then:
- **What goes:** your account, password hash, characters, sessions and game tokens, your op status and any ban. If you are in the lobby at the time, your connection is closed at your next action.
- **What stays:** the invite you redeemed keeps its label, the operator's note (such as who it was for), and when it was redeemed. Its code was only ever stored as a hash.
- **The database file:** SQLite overwrites deleted rows (`secure_delete`). `account delete` then folds the write-ahead log into the file and empties it, so the account is gone from both at once. If the server has the log in use at that moment, `account delete` says so, and older copies stay in the log until SQLite next restarts it (it then cuts the log back to nothing) or the server stops.
- **Backups** keep a copy until they expire.

The database file is readable by its owner only (mode 0600), since it holds the password hashes.

## This repository

GitHub hosts this repository, and [GitHub's privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement) covers what GitHub records when you visit it or contribute. The project adds no tracking of its own.
