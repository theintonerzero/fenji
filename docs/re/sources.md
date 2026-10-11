# External sources

Every outside source we take hints from (design spec §2 rule 2, §4 C). Only clearly licensed material with clean provenance: nothing derived from leaked server files, binaries, scripts or source.

## Used

| Source (URL) | Licence | Provenance checked | What we used it for | Date |
|---|---|---|---|---|

No external sources used yet.

## Rejected

Material refused because it is leaked server material, or derives from it. Nobody doing Fenji RE or protocol work may read it.

| Source | Why rejected | Exposure | Date |
|---|---|---|---|
| "GuJian Online (古剑奇谭网络版) Server Reverse Engineering Report" (`GuJianOL-Server-RE-Report.md`, distributed in a RaGEZONE zip) | Its table of contents shows an analysis of the game's server package (server binaries, start script, server-side scripts), which the developer never published | One Claude session, `018pTqyt…` (the Caphras → Fenji transition), read its table of contents only and used nothing. That session coordinated Fenji until about 23:00 AEDT on 2026-10-11, making process rulings on clean agents' evidence, and did no RE or protocol work apart from one diagnostic sandboxed start of the client (22:49 on 2026-10-11, [spike-0](spike-0.md) P0, not used for any finding). Coordination then passed to `014j8e76…`, a session that has not read it. The owner deleted the downloaded copy on 2026-10-11 | 2026-10-11 |
