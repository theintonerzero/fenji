# M2: summary

M2, transport and handshake, took the real client from the first byte to `WORK`. `fenji-proto` implements the client's transport as sans-I/O code, and `fenji-stub --speak` used it to run the real client through every rung:
- the hello;
- the auth message;
- a stored Oodle packet;
- `PROTOG2C_SET_NET_CONNECTED`;
- the client's first script message, decoded, on both login paths.

The Oodle Network question closed without an encoder: the client's decoder accepts stored packets. The archives opened too. A sandboxed helper decodes Oodle blocks with the client's own DLL, so `_index` names every archive file. The design tables turned out to ship as one pickle cache, which `archive dump` now writes out as JSON lines.

**Status (2026-10-12):** all seven Done-when items are met (item 2's emulator vectors by the plan's fallback, below). Release: v0.3.0, pre-approved by the owner for when the final review is clean and CI is green.

The documents:
- [`../protocol/transport.md`](../protocol/transport.md): the hello, the variants, keys and frames, the Oodle wrapper, the auth exchange, the inner messages, the runs (§9) and what follows auth (§10).
- [`../protocol/opcodes.md`](../protocol/opcodes.md): every `STC_PROTOCOL_*` and `CTS_PROTOCOL_*` value, and the script message ids used so far.
- [`../protocol/payloads.md`](../protocol/payloads.md): the client's pickle format, its LZ4 form, and the auth payload.
- [`m1-archives.md`](m1-archives.md) 3.4–3.5: Oodle blocks, and `_index`'s paths.
- [`m2-tables.md`](m2-tables.md): where the design tables live, the cache format, and the five dumped sheets.
- [`README.md`](README.md): the evidence norm for `gujianol.exe` (#18).

## Open questions answered

| From | Question | Answer | Where |
|---|---|---|---|
| spike-0 1.8 | Whether `AurogonEngine.dll`'s exports can name engine functions in `gujianol.exe` | **Partly.** The recipe and its limits are recorded. | `README.md` (spike-0 1.8 section) |
| spike-0 4.7 | What the client does with its Oodle LZ wrappers, and whether outgoing traffic passes through them | **Answered.** Four codecs (Kraken, Mermaid, Selkie, Leviathan) share one decompressor, FUN_14024E060. It reads each archive block's `u32` raw length and decodes the rest. The client's outgoing frames carry no Oodle data: they decode with the cipher and inner header alone. | `m1-archives.md` 3.4; `transport.md` §9 |
| spike-0 4.9 | Which serialiser the payloads use | **Answered.** The client's own pickle, LZ4-compressed from 128 bytes up. The table caches add table references and magic objects. | `payloads.md`; `m2-tables.md` §2 |
| spike-0 4.14 | Each opcode's number on the wire | **Answered.** `STC` 0–29 and `CTS` 0xFF, then 0–11. `STC_PROTOCOL_SCRIPT` and `CTS_PROTOCOL_GATE_SCRIPT` are verified on the wire. | `opcodes.md` |
| spike-0 4.23 | The hello and the variant choice | **Answered and run.** The hello's check is one word against `(K \| 0xC0000000) ^ w1 ^ w0`, with K = 0xE62F7B30 (`net_client_create`'s integer). The variants are w0 % 27 and w1 % 27, taken unsigned. | `transport.md` §2–3, §9 |
| spike-0 4.24 | What differs between the 27 variants | **Answered.** Eight cipher families. Fenji implements and verified AES-128 (variant 3). | `transport.md` §3 |
| spike-0 4.25–4.27 | The auth message: the 16 bytes, the 6 machine bytes, the payload | **Answered and run.** The 16 bytes are the dictionary digest, zero in every run. The 6 bytes are `SendARP`'s result, a failure marker in the sandbox. The payload is a pickle of 10 values: the ticket, account, user id, version, a machine fingerprint, the language and more. | `transport.md` §6, §9; `payloads.md` |
| spike-0 4.28 | The server's reply | **Run.** A stored `ok` moves the client to `QUEUE`. Any other body closes it with `CLOSED_AUTH`, and the reason is shown to the player. | `transport.md` 5.8, §9 |
| spike-0 4.30 | The scramble and keys | **Answered and run.** After each frame the key changes: server to client to `k + 0xD736`; client to server to `(k + 0x28CA) XOR k` after a data frame and to `(k + 0xB4FB) XOR k` after a time sync. A later, longer stored packet under the stepped key worked in m2-r4. | `transport.md` §4, §9 |
| login.md §5 | What the server must send first | **Answered and run:** the 64-byte hello. | `transport.md` §2 |
| login.md §5 | The Oodle Network data the client expects | **Answered and run:** a stored packet (raw length = packet length) passes the decoder, so the server needs no encoder. | `transport.md` 5.8 |
| login.md 2.7 | What `net_client_create`'s integer 3861871408 means | **Answered:** it is the hello's magic K. | `transport.md` §2 |

## Deviations from the spec

| # | Deviation (in the approved plan) | How it ended |
|---|---|---|
| D1 | `tools/oodle-helper` allows `unsafe` in one module | Kept, confined to `dll.rs`. CI now lints that file for the Windows target (#82). |
| D2 | The helper works on a content-addressed spool, not a job list | Kept, and hardened after review (#82). Inside the sandbox, `in/` is read-only. The host never writes through a link left in the spool, and accepts only regular files from `out/`. |
| D3 | `handshake::Server` stopped at `Queued` until the move to `WORK` was known | Closed in Task 10: `begin_work` sends `PROTOG2C_SET_NET_CONNECTED` (#81). |
| D4 | New crates: `aes`, `sha1`, `libloading` | Added as planned. `fenji-proto` also uses the workspace's `lz4_flex` for the pickle's compressed form, by a ledgered ruling (#81). |

Also by ledgered rulings:
- **Emulator vectors** (spec §7) weren't produced: Ghidra's `emulate_function` failed on every call in this setup. As the plan allowed, the expectations were derived by hand and checked with `openssl` and Python. The real client's frames, kept as golden fixtures, are the end-to-end check.
- **Amendments A and B** (Tasks 9 and 13) pin interfaces, vectors and tests, and their code was written test-first in the next task.
- **The tables:** the plan expected `.tab` files. They are one pickle cache, so Amendment B swaps the `.tab` parser for a cache reader (`m2-tables.md` §1).

## Done when

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | Transport written up | **Met.** `transport.md` covers every layer with evidence. `opcodes.md` lists every value, with its status. `payloads.md` covers the encoding. | #57, #65, #66 |
| 2 | `fenji-proto` | **Met.** All the §4 modules have unit tests, plus scrubbed golden fixtures from runs m2-r1 and m2-r4b. Emulator vectors were replaced by hand-derived, independently checked vectors (above). | #59–#62, #81 |
| 3 | The stub speaks | **Met.** In m2-r4 and m2-r4b (the launcher path) and m2-r5 (the server-list path), the client reached `WORK` and sent `GATE_SCRIPT` 402. The stub logged it decoded. The watchdog was clean in every run. | `transport.md` §9; #63, #66 |
| 4 | Oodle Network path | **Met.** R3 and its control m2-r3b: stored packets work, so gate G2 never opened. | `transport.md` 5.8, §9; #64, #72 |
| 5 | Archives (#51) | **Met.** The helper decodes Oodle blocks. `_index` gives 272,734 paths. The `.tab` format is read. `archive dump` writes items, skills, monsters, the XP curve and maps under `re/`. | `m1-archives.md` 3.4–3.5, `m2-tables.md`; #67–#69 |
| 6 | Loose ends | **Met.** #18 closed with a norm: a new finding cites an REA Evidence ID next to the pinned-exe address, and addresses alone stay acceptable for earlier findings and where REA has no function. M2's findings written after the norm carry their IDs with the exact calls: added in Task 14 to `transport.md` §10, `opcodes.md`, `payloads.md` and `m1-archives.md`, and in Task 13's review fixes to `m2-tables.md`. Spike-0 1.8 is answered in part. | `README.md`; #58, #70 |
| 7 | Records | **Met:** this document; "M2 update" notes in the parent spec's §6.2, §10 and §12. `sources.md` is unchanged: M2 used no outside source. | #70 |

## Gates and the owner's rulings

| Gate | Outcome |
|---|---|
| G1: the hello needs a secret | **Not hit.** The check needs only K, a constant in the client. |
| G2: the client rejects stored packets | **Not hit.** The owner had pre-decided a dev-only fallback (#72), which was closed unneeded. |
| G3: P2, the client's Oodle DLL for archives | **Used as approved:** dev analysis only, inside the sandbox, on the owner's machines. Server-side use would be a new gate. |
| G4: players on the `gj_uinfo` path (#54) | Stays with M3. |
| Runner exit 3 | **Never.** The watchdog stayed clean in every run. |

The owner also ruled, before leaving M2 to run:
- **A watchdog exit 3:** continue only on a proven non-AGT false positive, otherwise stop all runs. It never came up.
- **v0.3.0:** pre-approved once the final review is clean and CI is green.

Findings M3 inherits:
- **The server-list path's auth payload** is LZ4-compressed, and its mocked-SDK ticket is a hex JSON object (`payloads.md` P.6).
- **The design data:** 339 sheets in `all_tables.new_cache` (`m2-tables.md` §3).
- **#84:** bound the pickle decoder's memory before the server decodes client messages.
