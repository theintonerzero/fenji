# Security policy

Fenji is a small volunteer project, so there is no bug bounty and no team on call. Reports are still taken seriously and answered as soon as we can.

## Supported versions

Security fixes go into `dev` and ship in the next release, or in a hotfix release if they're urgent. Older releases don't get backports.

| Version | Supported |
|---|---|
| `dev` and the latest release | Yes |
| Older releases | No |

Fenji isn't a game yet, and it's meant for small private servers. v0.4's server speaks plain HTTP for its account API, and the launchers refuse to send a password over `http://` except to this machine, so hosting it beyond this machine needs a TLS proxy. From v0.5.0 a server can face the internet for the friends its host invites ([docs/hosting.md](docs/hosting.md)): its account API speaks TLS with the server's own key, whose pin invite links carry, or sits behind a proxy that holds a certificate. The game connection is a separate matter: it is readable and unauthenticated ([PRIVACY.md](PRIVACY.md), "The game connection"), and TLS on the API won't change that.

A server keeps what must stay private in its data folder, which it makes readable by its own user only (0700): the database, with the password hashes, and its backups; the private key it serves TLS with, `tls/key.pem` (0600); and the host console's socket, `console.sock` (0600). Anyone who can open the socket can run every console command, so attaching to a server's console takes the server's own user. The server refuses a key folder that other users can open, and the systemd unit in `deploy/systemd` runs it as a user of its own.

## Reporting a vulnerability

Report it privately through GitHub: open the repository's **Security** tab and choose **Report a vulnerability** (<https://github.com/theintonerzero/fenji/security/advisories/new>). Please don't open a public issue, pull request or discussion about it.

A good report says:
- what is affected (a crate, tool, script or document) and at which commit or release;
- how to reproduce it, ideally with a minimal input;
- what an attacker could do with it.

We'll acknowledge it within about a week, keep you posted while we fix it, and agree a disclosure date with you. We credit reporters in the advisory unless you'd rather we didn't.

## Scope

In scope is everything in this repository, for example:
- `fenji-proto` and every other parser of bytes that a peer or a file controls: frames, pickles, archives, table caches;
- the never-send list in `fenji-proto` (`NEVER_SEND`): any way to make a Fenji server send one of the messages it refuses;
- the account and launcher code (`fenji-auth`, `fenji-store`, `fenji-server`, `fenji-launcher`): password storage, tokens, rate limits, the server's own TLS key and the launchers' pins;
- the host console (`fenji-console`, and the server's console socket): what players' text can do to a host's terminal, and who can attach;
- the reference deployment in `deploy/` and [docs/hosting.md](docs/hosting.md);
- the sandbox and helper scripts under `scripts/spike/` and `tools/oodle-helper`, whose isolation the project relies on;
- CI and the release process.

Out of scope:
- **The game itself.** Swords of Legends Online's client, its anti-cheat driver, and the services of its developer, its publishers or Steam. Report problems there to their owners.
- **What a host can do to its own players.** The game client obeys server messages that write files and run code. Fenji's server never sends them, but a host who changes it could. Joining a server means trusting its host ([PRIVACY.md](PRIVACY.md#trusting-a-servers-host)).
- **Testing against live services.** Never point a modified or instrumented client, or any Fenji tool, at an official service. Test against your own Fenji stub or server only.
- **Anything that needs leaked server material.** Fenji is a clean-room project ([CONTRIBUTING.md](CONTRIBUTING.md)), so we can't accept or act on findings drawn from leaked server files or source.
