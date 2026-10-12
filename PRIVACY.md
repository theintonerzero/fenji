# Privacy

This page says what personal data Fenji's software handles and where it goes. It isn't a contract. The project itself runs no service: every Fenji server is run privately by someone who hosts it for the players they invite (the **operator**), and the operator is responsible for the data on that server.

Fenji isn't playable yet. The server, the account service and the launcher are planned for later milestones, so the parts below describe their design ([`docs/superpowers/specs/2026-10-11-fenji-design.md`](docs/superpowers/specs/2026-10-11-fenji-design.md) §5–§8). This page changes when they're built; its git history shows how.

## No telemetry

Fenji's own software sends nothing to the project or to anyone else. It has no analytics, crash reporters or update checks that phone home, and talks only to the server you point it at, and to Steam through DepotDownloader if you download your game client that way. The game client is another matter (below).

## What a Fenji server stores

- **Your account:** your username, your password as an argon2id hash (never the password itself), when the account was created and when you last signed in.
- **Invites:** the one-time codes the operator issues, each with an optional label the operator writes, such as who it's for.
- **Your characters:** their stats, inventory and skills, so your progress survives a restart.
- **Backups:** the reference setup (design spec §8) keeps nightly backups of the server's database for 14 days; each operator can choose otherwise.

It also handles short-lived tokens: a session token after you sign in, and a single-use game token, valid for 60 seconds, when you press Play.

Like any server, it sees the IP address you connect from, and its logs may record it. The operator decides how long logs and backups are kept.

## What Fenji never handles

- **Your Steam or Gameforge password.** You type those only into the services' own pages, or into DepotDownloader, which talks to Steam directly.
- **Payment details.** Fenji's software has no payment features.

## What the game client sends

When the game client logs in, its own login message includes, besides your account name and a ticket, its version, its language, a network hardware (MAC) address and a 20-byte machine fingerprint. That is the client's behaviour, not something Fenji adds. A Fenji server doesn't need the address or the fingerprint, and the design doesn't store them.

The game client is the game developer's software, not Fenji's, and Fenji can't speak for everything it does. It ships crash-reporting components (`AurCrashRpt64.dll`, `sentry.dll`, `crashpad_handler.exe`) that may upload a report when it crashes, and it loads the Steam and Gameforge platform libraries. In the sandboxed test runs where we checked, the crash helpers didn't start; a player's own sessions haven't been examined.

Developers who study the client run it inside a sandbox, against their own stub or server; play tests outside the sandbox happen only on the project's own Linux machine, with the client's network limited to a local Fenji server. Those runs' logs stay on the developer's machine, in a folder git ignores. Test fixtures in this repository, which come from such runs, have the address and fingerprint replaced with made-up values, and a test checks that they are.

## Deleting your data

Ask your server's operator. The design has no self-service deletion yet: an operator deletes an account by hand, and backups keep a copy until they expire.

## This repository

GitHub hosts this repository, and [GitHub's privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement) covers what GitHub records when you visit it or contribute. The project adds no tracking of its own.
