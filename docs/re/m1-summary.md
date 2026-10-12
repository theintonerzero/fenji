# M1: summary

M1, the data layer, made the client's own data readable and connected the real client to our stub. All 20,443 embedded scripts decode and decompile, and the archives' structure reads without decompression. The census shows how much server logic the scripts carry, and the client now finds and opens a session with fenji-stub. The archives' compressed content and the table dumps moved to M2 (#51), by the owner's ruling at the Oodle gate (#39).

**Status (2026-10-12):** all seven Done-when items are met, item 6 by the owner's ruling at the gate. Release: v0.2.0, with the owner's go-ahead.

The documents:
- [`m1-script-format.md`](m1-script-format.md): the embedded file system, the blob format, the Lua 5.3 dialect and its extra opcodes.
- [`m1-script-census.md`](m1-script-census.md): how much server-side logic the scripts carry.
- [`m1-archives.md`](m1-archives.md): the data archives, read without decompression.
- [`../protocol/login.md`](../protocol/login.md): how the client finds its server, and the first connection.

## Spike-0's open questions

| spike-0 | Question | Answer | Where |
|---|---|---|---|
| 6.4 | The archives' record format | **Answered.** A part is `ZFS\0` followed by a chain of `[IX]` index blocks. A record holds a SHA-1 key, an offset, a size, a CRC-16 and a status. Deleted records and reused space explain why spike-0's reading didn't tile the parts. | `m1-archives.md` §1–§2 |
| 6.7 | Whether any entry type is encrypted | **Partly.** Stored files are plain: their SHA-1s match their keys. Compressed files are carried to M2 (#51), by the owner's ruling at gate #39. | `m1-archives.md` §2, §5 |
| 6.9 | `_index`'s format | **Structure answered:** one Oodle blob of `<sha1 hex>\t<path>` lines, by the engine's own parser. Reading it is carried to M2 (#51). | `m1-archives.md` §3 |
| 6.16 | Which size field the chunk header drops | **Answered:** the `size_t` byte. | `m1-script-format.md` 3.3 |
| 6.19 | Whether the VM keeps the stock opcode numbers | **Answered:** opcodes 0–46 are stock. Of the extra opcodes 47–53, only 49, 50 and 53 occur. | `m1-script-format.md` 3.8, §4 |
| 6.21 | How much server-side logic the scripts carry | **Answered:** 17.6% of `data/scripts`' instructions run only on the server, mostly in combat. Two `server/` directories also ship whole. | `m1-script-census.md` |
| 8.19 | Why the interface is German | **Answered:** the interface language comes from the Gameforge SDK's game language, and the mocked SDK hard-codes `de-DE`, so no input can change it. A real SDK, or the `gj_uinfo` start-up branch that skips the SDK, would give English (M3). | `login.md` §4 |
| 8.37 | What gives the client its server | **Answered and run:** the `serverlist_url` setting (a `file://` list works), then the player's choice in the server-choice frame. The real client opened a session with fenji-stub that way. | `login.md` §2, §3, §6 |

## Deviations from the spec

| # | Deviation (accepted by the owner) | How it ended |
|---|---|---|
| D1 | Locating and decompressing live in `fenji-data`, shared with the archives | Done. `fenji-data` reads the blob format and the embedded file system (#40). `tools/archive` reuses it (#47). |
| D2 | Chunks are found through the client's registry, not a byte scan | Done, through the mount table found in Task 1 (plan amendment E1). All 21,512 files decode. |
| D3 | Done-when 1 counts registry entries, not 18,587 magic hits | Done. Task 1 then found that no entry is cut short (E2), so the list of entries the exe can't supply is empty. |
| D4 | New dependencies: `sha2`, and `mlua` for tests only | Added (#40, #42). Task 12 also added `sha1` (MIT or Apache-2.0) to `tools/archive`, by a ledgered ruling. |

## Done when

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | Lua tooling | **Met.** `luatool` lists all 21,512 entries (20,443 Lua chunks, none undecodable). It disassembles, normalises and decompiles all 20,443 chunks, with counts per area, and the login scripts read. | `m1-script-format.md` (Decompile results); #43–#45 |
| 2 | Login write-up | **Met.** | `login.md` (#41, #49) |
| 3 | First connection | **Met.** In run m1-c2, the real client opened a session with fenji-stub on port 7800. Neither side sent a byte, because the server speaks first. | `login.md` §6 (#49) |
| 4 | Census | **Met.** | `m1-script-census.md` (#46) |
| 5 | Runner | **Met.** The tracked runner and the AGT watchdog have tests and were reviewed and hardened. GE-Proton is pinned. Issue #17 was closed as a duplicate of #32. | #48 |
| 6 | Archives | **Met, by the owner's ruling.** The format is parsed as far as it goes without decompression. At the Oodle gate the owner chose (d), defer: the table dumps leave M1 for M2. | `m1-archives.md` (#47); #39; #51 |
| 7 | Records | **Met:** spike-0's questions above; "M1 update" notes in the parent spec's §10 and §12; the external decompiler (unluac) in `sources.md`. | this document; #45 |

The AGT driver stayed quiet through M1's runs, the first connection included. The watchdog saw no sign in any run. Runs m1-c1 and m1-c2 also logged every resource lookup, and none was of the driver's type `PE`.
