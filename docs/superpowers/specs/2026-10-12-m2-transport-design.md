# M2: Transport and handshake — Design Spec

**Status:** implemented in M2 (v0.3.0); see [`docs/re/m2-summary.md`](../../re/m2-summary.md).
**Parent:** [Fenji design spec](2026-10-11-fenji-design.md) §4, §6.2 and §10 (M2), the M0 findings in [`docs/re/spike-0.md`](../../re/spike-0.md) §4, and M1's [`docs/protocol/login.md`](../../protocol/login.md) and [`docs/re/m1-summary.md`](../../re/m1-summary.md).

## 1. Purpose and priority

M2 teaches our stub to speak the client's transport. The real client connected to fenji-stub in M1 and then waited in silence, because the server speaks first. M2 ends when the stub has taken the real client through its gate handshake and `fenji-proto` decodes the first packets the client sends after it.

As in M1, the priority is **the fastest route to the next real-client milestone**: here, the client in state `WORK`. The archive work moved from M1 (#51) runs alongside and never blocks the handshake.

M2 builds `fenji-proto`, extends the stub and adds analysis tooling. It doesn't create `fenji-server` (M3) and never changes the client.

### What we already know

From spike-0 §4 (static reading, **Inferred** unless marked):
- **The server speaks first.** The client waits for at least 64 bytes, checks them against constants (0x14029C3C3–0x14029C3E3; error 0x5F5E104 on a mismatch), then picks one of 27 stream variants in FUN_14029BA10 (4.23).
- **Auth.** The client then enters `IN_AUTH` and sends its first message: an inner header of type 0, 16 bytes from the shared-dictionary object, 6 bytes from `SendARP` (most likely the MAC address) and the payload `net_client_create` received (4.25). A 2-byte reply body `ok` moves it to `QUEUE`. Any other body becomes the close reason, with error 0x5F5E106 (4.28).
- **Inner header.** 4 bytes: a 24-bit length split across bytes 0, 3 and 2, and byte 1 the message type XOR the low byte of (length >> 1) (4.29).
- **Scramble.** A 16-bit key per direction (4.30). The client sends a time sync at most every 2 s (4.31).
- **Oodle.** The client imports Oodle Network's decoder but not its encoder (4.4–4.5).

From reading the receive loop during M2 planning (**Inferred**, pinned-exe addresses):
- **Frames.** FUN_14029B850 reads frames whose first `u16` is the body length XOR the receive key at ctx+0x1480. The key steps by −0x28CA per frame.
- **Time-sync frame.** A length of 0 means a 10-byte frame of four more scrambled `u16`s.
- **Every other frame** goes through:
  1. the variant's transform, a function pointer at ctx+0x3D8;
  2. then **unconditionally** the wrapper at 0x14029B0A0. The wrapper reads a raw-length varint (one byte below 0xFC, or `FC`/`FD`/`FE` followed by 2, 3 or 4 bytes) and calls `OodleNetwork1TCP_Decode` on the rest. If the decode fails, the connection closes with error 0x5F5E101.
- **So the server's `ok` must get past Oodle's decoder.** The Oodle Network question sits on M2's critical path.

(The receive step −0x28CA is 0xD736 modulo 2¹⁶, which agrees with spike-0 4.30. The send side uses 0x28CA at its flush, so both directions may step the same way; W1 confirms it, and what 0x4B05 does on the time-sync send.)

## 2. Decisions made at planning (owner, 2026-10-12)

| # | Question | Decision |
|---|---|---|
| P1 | The Oodle Network wire question | **Black-box test first.** The stub sends a "stored" frame (raw length equal to the body length) and we watch the client. Nobody reads the Oodle DLL. If the client accepts it, the wire needs no encoder and no gate. If it rejects it, a decision-gate issue goes to the owner, and only the work that needs server frames waits. |
| P2 | Route for #51 (archive decompression), deferred by gate #39 | **(c) the client's own `oo2core_6_win64.dll`, called from a helper inside the sandbox.** Approved for dev analysis on the owner's own machines only: never on the VPS, in the launcher or for players. Using it on the server would be a new gate. |
| P3 | Structure | **Static reading first, then a stub ladder.** Each transport layer is read in Ghidra, written up, implemented as a sans-I/O module with tests, then checked in one sandboxed run per rung. Ghidra's p-code emulator may run the client's own pure functions to produce golden vectors. |
| P4 | Login path for dev runs | **The launcher path** (`gj_uinfo`, `gj_gates=127.0.0.1:7800`, `gj_ticket`), as in run en-1: no list, no click, English. One final run on the server-list path with `--gf-sdk=mocked` checks that the handshake doesn't depend on the path. Whether players may use `gj_uinfo` stays gate #54 (M3). |
| P5 | Finish line | **The client in `WORK`, with its first packets decoded.** Character-list traffic is M3. |

## 3. Workstreams

| # | Workstream | Delivers | Depends on |
|---|---|---|---|
| W1 | Handshake reading | `transport.md` sections on the hello (FUN_14029C330's 64-byte check), the variant choice (FUN_14029BA10) and the 27 transforms (ctx+0x3D8), the scramble keys and steps in both directions, the auth message's layout, and the `ok` reply | — |
| W2 | `fenji-proto` core | The crate, with modules `hello`, `variant`, `scramble`, `frame`, `codec` and `header`. Unit tests from W1's reading; emulator vectors | W1, module by module |
| W3 | The stub speaks | `fenji-stub --speak` and rung runs R1–R3 (§5) | W2 |
| W4 | Post-auth reading | What moves the client from `QUEUE` to `WORK`; the dispatch tables (wire opcode values, native or Lua handlers); the payload serialiser; whether the server must answer time syncs | W1; runs alongside W2–W3 |
| W5 | First packets | Rung R4: the client reaches `WORK`. The `opcode` and `packet` modules; golden fixtures; `opcodes.md`; what the launcher path sends (#54's ticket question) | W3, W4, P1's outcome |
| W6 | Archives (#51) | The DLL helper, the `ExternalDecompressor`, `_index`, the `.tab` format and `archive dump` (§6) | — |
| W7 | Loose ends and records | #18 and spike-0 1.8 early, because both help naming in W1 and W4; the summary and spec updates at the end | — / everything |

The critical path is W1 → W2 → W3 → W5. W4, W6 and W7's early part start at once. If R3 fails, W3's later rungs and W5 wait on the gate while W4, W6 and W7 go on.

### W1's first question: can a server we write produce an accepted hello?

The hello's check is read first, before anything is built. If it needs a secret only the official servers held (a signature or a keyed MAC whose key isn't in the client), then no server of ours can pass it without patching the client. That is a ground-rule-6 stop: work halts and a decision-gate issue goes to the owner.

### W1's second question: do the variants need the shared dictionary?

The client trains its Oodle state from `data/core/share/gate_dictionary.bin`, which lives in the archives (spike-0 4.19–4.21). If any variant transform also depends on that file, W3 depends on W6, and the plan reorders.

## 4. `fenji-proto`

A new crate, `crates/fenji-proto`. It does no I/O and has no `tokio`: byte buffers go in, and byte buffers and events come out. It depends on nothing internal (parent spec §6.1).

| Module | Contents |
|---|---|
| `hello` | Builds and validates the 64-byte server hello, and says which bytes choose the variant |
| `variant` | `enum Variant` with 27 values, and the transform for each variant we implement. Only the variants our server chooses get implemented; the others are documented in `transport.md` and return `Err(Unsupported)` |
| `scramble` | `Scrambler { key, step }`, one per direction, with the constants W1 assigns |
| `frame` | Outer stream framing: the scrambled `u16` length, the zero-length time-sync frame, and the send side's 0x800-byte chunking |
| `codec` | The Oodle wrapper's raw-length varint, and a `StreamCodec` trait. `StoredCodec` (raw length = body length) is its only implementation in M2; a real encoder comes only through a gate |
| `header` | The 4-byte inner header |
| `handshake` | The server side's sans-I/O state machine (`AwaitAuth → Queued → Work`). It consumes received bytes and returns bytes to send plus `Event`s |
| `opcode`, `packet` | `#[non_exhaustive] enum Opcode`; unknown opcodes decode to `Packet::Unknown { opcode, bytes }` |

**Errors and limits:**
- Malformed input returns an error; nothing panics.
- Lengths are checked before any allocation (a 24-bit length caps at 16 MiB).
- Every type that stands for a wire structure cites its evidence in a doc comment: pinned-exe addresses, or REA Evidence IDs where #18 makes REA usable (the norm #18 settled: [`docs/re/README.md`](../../re/README.md#the-norm)).

**Dependencies:** `thiserror` (already in the workspace). The crate adds no new crate unless the plan justifies one.

## 5. The stub and the rung runs

### `fenji-stub --speak`

- `--speak` wires `fenji_proto::handshake` into the existing session loop.
- The raw hexdump log stays as it is: it remains the ground truth. A decoded `.events` log (states, frames, headers, opcodes) is written next to it.
- Without `--speak`, the stub stays the silent listener it is now.
- An integration test runs a fake client, built from `fenji-proto`, against `--speak` over loopback in CI.

### Rungs

Each rung is one run through `scripts/spike/run-client.sh` (sandbox options `--no-root --x11 N --cpu-topology`, the watchdog armed) with a predicted outcome written down before the run. Exit 3 from the runner stops all runs and is reported, as in M1.

| Rung | The stub sends | Passes when |
|---|---|---|
| R1 | the hello | the client accepts it: no close with 0x5F5E104, and it sends its auth message |
| R2 | the hello, then reads the auth message | the auth message decodes with `fenji-proto` (scramble, header, layout), and its scrubbed form becomes a fixture |
| R3 | `ok` in a stored frame | the client moves to `QUEUE`: no close with 0x5F5E101. **This is P1's test.** A rejection files the Oodle-wire gate |
| R4 | whatever W4 says moves the client from `QUEUE` to `WORK` | the client sends its first `WORK` packets, and they decode to an opcode and payload |
| R5 | R4 again, on the server-list path with `--gf-sdk=mocked` | as R4 (P4) |

If a rung fails, we go back to the static reading. A rung's run count isn't capped, but each run records its prediction, the outcome and the evidence in `re/m2/runs.md`.

## 6. Archives (#51, route c)

- **`tools/oodle-helper`** is a small Rust program we write, built for `x86_64-pc-windows-gnu`.
  - It loads the client's `oo2core_6_win64.dll` at run time from `$FENJI_CLIENT_PATH/bin64` (through `libloading`, ISC licence) and calls only `OodleLZ_Decompress`.
  - The DLL is never copied into the repo or into `re/`.
- **It runs only inside the sandbox,** through `isolated-run.sh` under GE-Proton: loopback-only, non-root, binding only the directories that belong to the run.
  - It never starts `gujianol.exe`, so the AGT watchdog isn't needed.
- **Batch mode.** Each Wine start costs seconds, and about 210,000 files are codec 4.
  - The helper takes a job list (archive part, offset, compressed size, raw size).
  - It writes each output to a content-addressed cache, `re/oodle-cache/<sha1>`. The SHA-1 is the record key (`m1-archives.md` §2), so every output checks itself.
- **`fenji-data`** gets an `ExternalDecompressor` behind the existing `Decompressor` trait. It reads the cache and queues misses for the next helper batch.
- **Then:**
  1. decode `_index` and map its `<sha1 hex>\t<path>` lines to the archives' records;
  2. read the `.tab` table format (Ghidra, from the engine's reader);
  3. add `archive dump` for items, skills, monsters, the XP curve and maps, with output only under `re/`.
- **New pins,** in one `chore(deps)` commit: the Rust target `x86_64-pc-windows-gnu` and the MinGW-w64 linker (Fedora `mingw64-gcc`, Homebrew `mingw-w64`), in `scripts/versions.env`, bootstrap and CLAUDE.md's table.
- **The helper is a tool, not a server component.** No Fenji crate links it or calls the DLL outside the sandbox.

## 7. Testing

- **Unit tests** per `fenji-proto` module, from the static reading.
- **Emulator vectors.** Ghidra's `emulate_function` runs the client's own scramble and transform functions on test inputs. Each vector in a test cites the function's address. Ghidra's script execution stays off (`GHIDRA_MCP_ALLOW_SCRIPTS` unset), and its `debugger_*` tools are never used on the client.
- **Golden fixtures** from the rung runs, minimal and scrubbed before commit:
  - the auth message carries the machine's MAC address (`SendARP`) and a machine fingerprint;
  - those fields are replaced with synthetic values, and the scramble and transform are re-applied;
  - a test checks that the scrubbed fixture still decodes.

  Raw stub logs stay in `re/`.
- **The stub's integration test** (§5) runs in CI.
- **Archive tests** use synthetic data built by the tests, plus a local-only check gated on `FENJI_CLIENT_PATH` and the helper's cache.

## 8. Gates and stop points

| # | When | What happens |
|---|---|---|
| G1 | W1 finds the hello needs a secret we can't hold | Ground-rule-6 stop: a decision-gate issue, and all handshake work waits |
| G2 | R3: the client rejects the stored frame | A decision-gate issue on the Oodle wire (options then: a clean-room encoder from analysing the decoder, the client DLL's encoder, or other routes W1/W4 find). W3's later rungs and W5 wait; the rest goes on |
| G3 | P2's approval | Recorded on #39 and #51. Dev analysis only; server-side use is a new gate |
| G4 | #54, player use of `gj_uinfo` | Stays with M3 |
| — | Runner exit 3 | All runs stop and the event is reported |

## 9. Process

- One plan: `docs/superpowers/plans/2026-10-12-m2-transport.md`. Each task becomes a `Task` issue in milestone M2.
- Execution follows the owner's choice when the plan is approved.
- Rulings made without the owner go in the ledger for ratification. Decision gates stop the affected work and go to the owner as issues.
- **Clean-room hygiene (CLAUDE.md):** Ghidra work, reading disassembly or decompiled output, and every client run go only to agents that have never read rejected material. Writing `fenji-proto` from the documented findings is ordinary code work.
- **Evidence:** pinned-exe addresses, plus REA Evidence IDs where #18 makes REA usable ([`docs/re/README.md`](../../re/README.md#the-norm)). `transport.md` separates **Observed**, **Inferred** and **Unknown**.

## 10. Done when

1. **Transport written up.** `docs/protocol/transport.md` covers the hello, the variants, the scramble, the frames, the Oodle wrapper, the auth exchange, the inner header and the payload encoding, with evidence. `docs/protocol/opcodes.md` gives the wire value of every `STC_PROTOCOL_*` and `CTS_PROTOCOL_*` opcode, with its status (`named`, `schema`, or `verified` for those seen on the wire).
2. **`fenji-proto`** exists as §4 describes, with unit tests, emulator vectors and scrubbed golden fixtures from real-client runs.
3. **The stub speaks.** In a sandboxed run (R4), the real client gets past auth, reaches `WORK`, and its first packets are logged decoded. R5 repeats it on the server-list path. The watchdog stays clean.
4. **The Oodle Network path is decided:** R3's result is recorded, and if it failed, gate G2 is decided.
5. **Archives (#51):** the helper, `_index` decoded, the `.tab` format read, and `archive dump` for items, skills, monsters, the XP curve and maps, output only in `re/`.
6. **Loose ends:** #18 is closed with a working REA recipe for `gujianol.exe` or a recorded decision that Ghidra addresses are the norm, in `docs/re/README.md`. Spike-0 1.8 (`AurogonEngine.dll`'s exports) is answered.
7. **Records:**
   - `docs/re/m2-summary.md`;
   - "M2 update" notes in the parent spec's §6.2, §10 and §12;
   - `docs/re/sources.md` updated if any outside source was used.

Release: v0.3.0, with the owner's go-ahead.

## 11. Out of scope

- `fenji-server`, accounts, the launcher and the ticket handoff (M3).
- Character-list and later lobby traffic (M3).
- Any Oodle encoder, unless gate G2 decides one.
- Any use of the Oodle DLL outside the owner's sandbox.
- Any change to the client.
