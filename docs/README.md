# Fenji docs

| Path | What it holds |
|---|---|
| [`superpowers/specs/2026-10-11-fenji-design.md`](superpowers/specs/2026-10-11-fenji-design.md) | The project design: ground rules, architecture, milestones. Start here. |
| `superpowers/plans/` | One implementation plan per milestone (M0–M5), written before that milestone starts. |
| [`development.md`](development.md) | Machine setup (macOS/Linux), branches, Conventional Commits, SemVer, changelog, releases. |
| [`re/README.md`](re/README.md) | Reverse-engineering workflow: pinned build, analysis setup, sandboxed client runs, what may be committed. |
| [`re/sources.md`](re/sources.md) | External sources we used, and sources rejected for clean-room reasons. |
| [`re/pinned-build.md`](re/pinned-build.md) | The exact SoLO build Fenji targets (AppID, depot manifest, exe SHA-256). |
| [`re/spike-0.md`](re/spike-0.md) | M0 spike: static triage of the pinned client, runtime observation, conclusions. |
| [`protocol/README.md`](protocol/README.md) | Our protocol findings (transport, opcode ledger, payloads) and their format. |

Ground rules (full text in the design spec, §2): legitimate copies only; clean room, no leaked server material; no game content in the repo; never touch live official services with modified clients; private and non-commercial.
