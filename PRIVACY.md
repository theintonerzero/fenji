# Privacy

This page says what personal data Fenji's software handles and where it goes. It isn't a contract. The project itself runs no service: every Fenji server is run privately by someone who hosts it for the players they invite (the **operator**), and the operator is responsible for the data on that server.

Fenji isn't playable yet. From v0.4.0 a Fenji server runs accounts and the character lobby, and the command-line launcher starts the client on Linux; the game world comes later. Where this page describes what isn't built yet, it follows the design ([`docs/superpowers/specs/2026-10-11-fenji-design.md`](docs/superpowers/specs/2026-10-11-fenji-design.md) §5–§8). This page changes as Fenji does; its git history shows how.

## No telemetry

Fenji's own software sends nothing to the project or to anyone else. It has no analytics, crash reporters or update checks that phone home, and talks only to the server you point it at, and to Steam through DepotDownloader if you download your game client that way. The game client is another matter (below).

On Linux the launcher starts the client through [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher), which has network habits of its own:
- it checks for a network route, with a UDP `connect` to `1.1.1.1` that sends nothing;
- it downloads its Steam Linux Runtime from Valve's servers (`repo.steampowered.com`) on first use, and asks them later whether it has an update;
- when you don't name a Proton build with `--proton` (or `$PROTONPATH`), it checks GitHub for the latest UMU-Proton each time it starts, and downloads it when it's new.

Those requests come from umu, not from Fenji. They carry nothing about your account, though like any request they show your IP address.

## What a Fenji server stores

- **Your account:** your username, your password as an argon2id hash (never the password itself), when the account was created and when you last signed in.
- **Invites:** the one-time codes the operator issues, each with an optional label the operator writes, such as who it's for.
- **Your characters:** for now their name, class, body type, level and appearance (voice, face, outfit and portrait), and when they were created, so they survive a restart. Stats, inventory and skills join them in later milestones.
- **Tokens, as hashes only:** invite codes, session tokens and game tokens are stored as SHA-256 hashes with their times, never as the codes or tokens themselves.
- **Backups:** the reference setup (design spec §8) keeps nightly backups of the server's database for 14 days; each operator can choose otherwise.

It also handles short-lived tokens: a session token after you sign in, and a single-use game token, valid for 60 seconds by default, when you press Play.

Like any server, it sees the IP address you connect from, and its logs may record it. The operator decides how long logs and backups are kept.

## The game connection

The game client's connection to a Fenji server is readable and unauthenticated. Anyone on the network path between you and the server can read what crosses it, and could pose as the server. That is how the game client was built, and Fenji can't change it without changing the client, which it never does. So:
- **No password crosses it.** Your password goes only to the server's account API; the game connection carries a ticket that works once, for one account, and expires after 60 seconds by default.
- **Someone on that path could still take over your session.** If that matters to you, play over a VPN with your friends, or on a network you trust.

## What Fenji never handles

- **Your Steam or Gameforge password.** You type those only into the services' own pages, or into DepotDownloader, which talks to Steam directly.
- **Payment details.** Fenji's software has no payment features.

## What the game client sends

When the game client logs in, its own login message includes, besides your account name and a ticket, its version, its language, a network hardware (MAC) address and a 20-byte machine fingerprint. In our test runs, up to the character screens, the client that Fenji's launcher started made one connection, to the Fenji server, and looked no names up. It also tried to read a local file for a server list, which wasn't there; that uses no network. That is the client's behaviour, not something Fenji adds. A Fenji server doesn't need the address or the fingerprint, and the design doesn't store them.

The game client is the game developer's software, not Fenji's, and Fenji can't speak for everything it does. It ships crash-reporting components (`AurCrashRpt64.dll`, `sentry.dll`, `crashpad_handler.exe`) that may upload a report when it crashes, and it loads the Steam and Gameforge platform libraries. In the sandboxed test runs where we checked, the crash helpers didn't start; a player's own sessions haven't been examined.

Developers who study the client run it inside a sandbox, against their own stub or server; play tests outside the sandbox happen only on the project's own Linux machine, with the client's network limited to a local Fenji server. Those runs' logs stay on the developer's machine, in a folder git ignores. Test fixtures in this repository, which come from such runs, have the address and fingerprint replaced with made-up values, and a test checks that they are.

## Deleting your data

Ask your server's operator. There is no self-service deletion yet. The operator runs `fenji-server account delete <username>`, and then:
- **What goes:** your account, password hash, characters, sessions and game tokens. If you are in the lobby at the time, your connection is closed at your next action.
- **What stays:** the invite you redeemed keeps its label, the operator's note (such as who it was for), and when it was redeemed. Its code was only ever stored as a hash.
- **The database file:** SQLite overwrites deleted rows (`secure_delete`). The write-ahead log can still hold them until SQLite next checkpoints it: when the log reaches about 4 MB, or when the server stops.
- **Backups** keep a copy until they expire.

The database file is readable by its owner only (mode 0600), since it holds the password hashes.

## This repository

GitHub hosts this repository, and [GitHub's privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement) covers what GitHub records when you visit it or contribute. The project adds no tracking of its own.
