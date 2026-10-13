# Security policy

Fenji is a small volunteer project, so there is no bug bounty and no team on call. Reports are still taken seriously and answered as soon as we can.

## Supported versions

Security fixes go into `dev` and ship in the next release, or in a hotfix release if they're urgent. Older releases don't get backports.

| Version | Supported |
|---|---|
| `dev` and the latest release | Yes |
| Older releases | No |

Fenji isn't playable yet. Until a release says otherwise, nothing in this repository is meant to face the internet. v0.4.0's server speaks plain HTTP for its account API. `fenji-launch` refuses to send a password over `http://` except to this machine, so hosting beyond it needs a TLS proxy, which M4 sets up. The game connection is a separate matter: it is readable and unauthenticated ([PRIVACY.md](PRIVACY.md), "The game connection"), and TLS on the API won't change that.

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
- account and launcher code once it exists: password storage, tokens, rate limits;
- the sandbox and helper scripts under `scripts/spike/` and `tools/oodle-helper`, whose isolation the project relies on;
- CI and the release process.

Out of scope:
- **The game itself.** Swords of Legends Online's client, its anti-cheat driver, and the services of its developer, its publishers or Steam. Report problems there to their owners.
- **Testing against live services.** Never point a modified or instrumented client, or any Fenji tool, at an official service. Test against your own Fenji stub or server only.
- **Anything that needs leaked server material.** Fenji is a clean-room project ([CONTRIBUTING.md](CONTRIBUTING.md)), so we can't accept or act on findings drawn from leaked server files or source.
