# M2: Transport and Handshake Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Take the real client through its gate handshake with our stub, decode the first packets it sends in state `WORK` with `fenji-proto`, settle the Oodle Network question, and decompress the archives (#51) with the client's own Oodle DLL in a sandboxed helper.

**Architecture:**
- **`crates/fenji-proto`** (new) is the transport with no I/O: the hello, the per-direction AES variant, the frame keys, frames, the Oodle wrapper's raw-length prefix (`StoredCodec` only), the 4-byte inner header, and two state machines: `handshake::Server` and, for tests and the later bot client, `client::Client`.
- **`tools/stub`** gains `--speak`, which drives `handshake::Server` over each connection. It keeps the raw hexdump log and adds a decoded `.events` log.
- **Client runs** climb a ladder of rungs R1–R5 through `scripts/spike/run-client.sh`, each with a prediction written down first.
- **Archives:** `tools/oodle-helper` (new, Windows target) calls `OodleLZ_Decompress` in the client's `oo2core_6_win64.dll` under Wine inside `isolated-run.sh`. `fenji-data` gains an `OodleSpool` decompressor that reads its outputs.

**Tech Stack:**
- Rust 1.97.1 (edition 2024).
- `aes` 0.9.3 (RustCrypto, MIT OR Apache-2.0; `cipher` 0.5.2) in `fenji-proto`.
- `sha1` 0.11.0 (already in the workspace) in `fenji-data`.
- `libloading` 0.9.0 (ISC) in `tools/oodle-helper`, Windows only.
- `x86_64-pc-windows-gnu` with MinGW-w64; GE-Proton11-7's Wine for the helper.
- `tokio` in the stub only; Ghidra 12.1.4 through the `ghidra` MCP (read-only, plus `emulate_function`).

**Spec:** [`docs/superpowers/specs/2026-10-12-m2-transport-design.md`](../specs/2026-10-12-m2-transport-design.md), which argues from the parent [`2026-10-11-fenji-design.md`](../specs/2026-10-11-fenji-design.md), [`docs/re/spike-0.md`](../../re/spike-0.md) §4 and [`docs/protocol/login.md`](../../protocol/login.md). Executors read the spec and this plan together.

## Global Constraints

Every task's requirements implicitly include these.

- **Ground rule 3: no game content in git.** That covers stub logs, captures, decoded archive files, `_index` paths, table dumps and disassembly. They live only in the git-ignored `re/`. Never bypass `scripts/check-forbidden-files.sh`.
- **Fixtures from client runs are scrubbed before commit** (spec §7). The machine's MAC address (the 6 `SendARP` bytes) is replaced with `02 00 00 00 00 01`, and the auth payload, until its fields are known (Task 10), with zero bytes of the same length. A test asserts that the committed fixture holds the synthetic values.
- **Clean room** (ground rule 2, CLAUDE.md "Clean-room hygiene"):
  - Ghidra work, reading disassembly or decompiled output, and every client or helper run are done only by a session or agent that has never read material rejected in `docs/re/sources.md`. The executing session confirms that before Tasks 1, 2, 8, 9, 10, 11 (Step 6), 12 and 13.
  - `docs/` holds prose and tables only: no decompiled code, no disassembly listings. Addresses, constants, enum names, function names and file names are fine.
  - Tag every finding **Observed** / **Inferred** / **Unknown**, citing pinned-exe addresses (or REA Evidence IDs once Task 2 settles #18).
  - Nobody reads `oo2core_6_win64.dll`'s code. The Oodle questions are answered black-box (R3) or from the client's own call sites.
- **Ground rule 6 gates stop work:** client patching (on disk or in memory), circumventing protection, anything that would load the AGT driver, and Oodle outside the scope the owner approved (P2: the client's DLL, called from `tools/oodle-helper`, inside the sandbox on the owner's machines, for dev analysis only). Stop points G1–G2 (spec §8) open a `Decision gate` issue and stop only the work that depends on them.
- **Ground rule 4: client runs** happen only through `scripts/spike/run-client.sh` (sandbox options `--no-root --x11 N --cpu-topology`, watchdog armed), against `fenji-stub`. Exit 3 is a gate event: stop all runs and report. The helper runs only through `scripts/spike/run-oodle-helper.sh`, which uses `isolated-run.sh --no-root`.
- **Rust conventions:**
  - Rust 1.97.1, edition 2024, `version.workspace = true`, `publish = false`.
  - `unsafe_code = "forbid"` everywhere except `tools/oodle-helper` (deviation D1).
  - Dependencies are declared once in the root `[workspace.dependencies]`.
  - `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` must pass.
- **`fenji-proto`:**
  - No I/O, no `tokio`, no allocation sized by a peer before a bound check.
  - Malformed input returns `fenji_proto::Error`; nothing panics on peer bytes.
  - It depends on nothing internal (parent spec §6.1).
  - Every wire structure's doc comment cites its evidence (addresses below).
- **Licences:** linked crates must pass `cargo deny check` (MIT, Apache-2.0, BSD-2/3, ISC, Unicode-3.0, Zlib). GE-Proton and the MinGW linker are run, never linked.
- **Pinned build:** `bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`, build 10015727.
- **Process:**
  - Conventional Commits with scopes `proto`, `data`, `tools`, `re`, `docs` and `deps`.
  - One issue → one branch → one PR per task (`Closes #N`), squash-merged into `dev` once CI is green and the task review is clean (owner's standing instruction).
  - `main`, tags and releases need the owner's go-ahead.
  - Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; PR bodies end with the Claude Code line.

## Review Focus

These are the five inputs most likely to hurt a user that the spec implies but doesn't spell out. Each has its pinning test in the task that owns the code.

1. **Hostile or garbage bytes from a peer:** a header length near 16 MiB, a `0xFF` raw-length prefix, a frame cut anywhere, junk instead of a hello. Each must give a typed error or "need more bytes", bounded memory, and no panic. Tests: Task 3 `raw_len_ff_is_refused` and `truncated_prefix_is_an_error`; Task 6 `oversized_inbound_message_is_refused` and `junk_never_panics`.
2. **TCP splitting and coalescing.** Bytes may arrive one at a time, or a hello and the first frames in one read. Both must give the same events. Tests: Task 4 `partial_frames_wait_and_keep_the_key`; Task 6 `byte_at_a_time_matches_one_read`.
3. **Messages larger than one frame** (over 0x800 bytes) in both directions, and a frame boundary inside a message header. Tests: Task 6 `large_messages_cross_frames_both_ways`.
4. **Fixture privacy.** A committed fixture must never hold the owner's MAC address or machine fingerprint. Tests: Task 8 `fixture_is_scrubbed`.
5. **Oodle spool integrity.** A partial, truncated or wrong-length output from the helper must never be accepted as decoded data, and two different blocks must never share a spool name. Tests: Task 11 `short_output_is_not_accepted`, `name_binds_raw_len` and `pending_input_is_written_once`.

## Planning findings (read-only Ghidra probes, 2026-10-12)

These are the facts the code tasks build on. Task 1 re-verifies each one and writes it up with evidence in `docs/protocol/transport.md`. Every tag here is **Inferred** (code reading), except where it says **Observed**.

**F1. The hello (FUN_14029C330, state `INIT`).**
- The client waits for 64 bytes and reads them as 16 little-endian `u32` words `w[0..16]`.
- It accepts them when `w[(w1 % 14) + 2] == (K | 0xC000_0000) ^ w1 ^ w0`. Otherwise it closes with error 0x5F5E104, state 6.
- `K` is ctx+0x50, set by FUN_14029A900 from `net_client_create`'s integer argument, 3861871408 = 0xE62F7B30 (login.md 2.6–2.7). Its top two bits are already set.
- **No secret is involved, so G1 doesn't trigger.**

**F2. Variants (FUN_14029BA10).**
- The 0x10C8-byte context at ctx+0x3C0 (client receive) is selected by `w0 % 27`; the one at ctx+0x1488 (client send) by `w1 % 27`.
- Each context starts with four function pointers: `[0]` a common one (FUN_14008E470), `[1]` `init(ctx, hello, 64)`, `[2]` the send transform, `[3]` the receive transform.
- They form 8 families of LibTomCrypt ciphers (the strings `src\ciphers\*.c`, **Observed**):

  | Family | Cases (`w % 27`) | Cipher | Setup / encrypt / decrypt |
  |---|---|---|---|
  | AES | 3, 10, 19 | AES (`aes.c`) | FUN_14008EBC0 / FUN_14008F3B0 / FUN_14008FA10 |
  | Blowfish | 2, 6, 22 | Blowfish | FUN_14008D5B0 / … |
  | RC5 | 1, 5, 9, 13 | RC5 | FUN_14008DDA0 / … |
  | RC6 | 4, 11, 17, 21 | RC6 | FUN_14008E4F0 / … |
  | Twofish | 0, 20, 23 | Twofish | FUN_140090930 / … |
  | CAST5 | 7, 15 | CAST5 | FUN_1400913E0 / … |
  | Anubis | 8, 12, 14, 16, 18, 24, 26 | Anubis | FUN_140092DC0 / FUN_140093780 / … |
  | XTEA | 25 (`default`) | XTEA | FUN_1400900C0 / FUN_140090200 / FUN_1400903C0 |

- **Case 3** (init FUN_140299C50) folds the hello to a 16-byte key and runs AES setup: **AES-128**. Case 10 (FUN_140299A70) uses a 32-byte key: AES-256. Task 1 reads case 19 (FUN_140299A10).
- **Transforms.** The AES send transform (FUN_140299B90) encrypts each whole 16-byte block in place (ECB); the receive transform (FUN_140299AD0) decrypts. Each then XORs every remaining byte at position `p` with `((end - p) as u8) ^ 0xC6`.
- **Key fold (FUN_14029BE20):** `key[i % n] ^= hello[i]` over all 64 hello bytes, starting from zero (`n` = key length).
- **The server chooses both variants.** Fenji uses case 3 in both directions. Its server encrypts what it sends (the client decrypts with `[3]`) and decrypts what it receives (the client encrypted with `[2]`).

**F3. Frame keys** (both in the variant contexts, so 0 once the hello is handled):
- **Server → client** (FUN_14029B850):
  - Each frame starts with a `u16` LE `len ^ k`.
  - `len == 0` is a time-sync frame: four more `u16`s, each XORed with `k`.
  - Otherwise `len` bytes of body follow.
  - After every frame, `k = k + 0xD736` (wrapping; `ADD [RBX+0x1480], R15W` at 0x14029B904 and 0x14029B964, R15 = 0xD736 at 0x14029B88F, **Observed** in the listing).
- **Client → server:**
  - A data frame (FUN_14029C600) is the `u16` `len ^ k` followed by the transformed chunk, at most 0x800 bytes; then `k = (k + 0x28CA) ^ k` (0x14029C6C0–0x14029C6CD).
  - A time-sync frame (FUN_14029A5B0, sent in every state after the hello, at most every 2,000 ms) is `k`, then the four `u16` words of the client's millisecond clock, each XORed with `k`; then `k = (k + 0xB4FB) ^ k`.

**F4. Server frames always go through Oodle.** After its transform, every non-time-sync server frame body goes to FUN_14029B0A0:
- It reads a raw length: one byte below `0xFC`; or `FC` + `u16` LE; `FD` + 24-bit LE; `FE` + `u32` LE. `FF` gives no prefix and a raw length of 0.
- It then calls `OodleNetwork1TCP_Decode` on the rest. A failure closes with error 0x5F5E101.
- The decoded bytes are appended to the inbound message stream (ctx+0x390).
- Client data never passes through Oodle.

**F5. Auth (FUN_14029C330, then FUN_14029ADD0, state `IN_AUTH`).**
- **The client's message:** a 4-byte inner header of type 0 and length `22 + payload_len`, then 16 bytes from the object at DAT_1431FC8E0, then 6 bytes, then the `net_client_create` payload (at most 0x4000 bytes; FUN_14029A900).
- **The 6 bytes** are:
  - the `SendARP` MAC address of the socket's local IPv4 address;
  - or `FF 00` plus the `SendARP` error as `u32` LE, if it fails;
  - or `FE 00 00 00 00 00`, if `getsockname` fails.
- **The reply.** In `IN_AUTH` the client takes the first decoded message. If its length is 2 and its body is `ok` (`0x6B6F` LE), the client goes to state 2 (`QUEUE`). Anything else, up to 255 bytes, becomes the close reason (error 0x5F5E106, state 7).

**F6. Inner header** (spike-0 4.29, confirmed in F5's code): `[len >> 16, ty ^ (len >> 1) as u8, len as u8, len >> 8]`, where `len` is the body length excluding the header.

**F7. After `QUEUE`.**
- States 2 and 3 both receive and flush. No code in the poll (FUN_14029ADD0) moves 2 to 3.
- The Lua binding `net_client_dispatch` (FUN_140222AD0) turns a posted state 2 into 3 under a condition on DAT_1431F3ED0. Task 9 reads this.
- The other bindings are `net_client_create`, `net_client_debug` and `net_client_get_server_time` (**Observed** strings).

**F8. The client's Oodle LZ call site** (FUN_14024E060, one of the four wrappers in spike-0 4.6):
- `OodleLZ_Decompress(comp + 4, comp_len - 4, out, raw_len = *(u32*)comp, 1, 0, 0, 0, 0, 0, 0, 0, 0, 3)`: 14 arguments.
- So the wrapper's blocks carry a `u32` raw length first.
- Whether the archives' codec-4 blocks use this wrapper (they begin `8C 06`, m1-archives) is Task 11's Step 6.

**F9. Crates checked during planning.** In a throwaway crate, `aes` 0.9.3 (`KeyInit::new_from_slice`, `BlockCipherEncrypt::encrypt_block`, `BlockCipherDecrypt::decrypt_block`) reproduced FIPS-197 C.1: `00112233…eeff` under key `000102…0f` gives `69c4e0d86a7b0430d8cdb78070b4c55a`. `libloading` 0.9.0's `Library::new` and `Library::get` are `unsafe fn`; its licence is ISC.

## Deviations from the spec (for the owner's review of this plan)

| # | Deviation | Why |
|---|---|---|
| D1 | `tools/oodle-helper` doesn't inherit the workspace's `unsafe_code = "forbid"`. It sets `unsafe_code = "deny"`, with one `#[allow(unsafe_code)]` module for the DLL call | Calling a C function in a DLL needs `unsafe`. The allowance is confined to one small module of one Windows-only tool that no Fenji crate links |
| D2 | The helper reads a content-addressed spool, not "a job list (archive part, offset, compressed size, raw size)" (spec §6). `fenji-data` writes each missing compressed block to `re/oodle-spool/in/<sha1>-<rawlen>.bin`; the helper writes `out/<same name>` | No archive paths or offsets cross into the sandbox, the helper needs no archive parsing, and every output is checked by name and length on the host |
| D3 | `handshake::Server` ends at `Queued` in Task 6; the move to `Work` arrives with Task 10's amendment | What moves the client from `QUEUE` to `WORK` is unknown until Task 9 (F7) |
| D4 | New linked crates: `aes` (fenji-proto), `sha1` (fenji-data), `libloading` (oodle-helper) | Spec §4 asks the plan to justify any new crate: an audited AES instead of a hand-written one, the workspace's existing SHA-1, and the standard DLL loader |

## File map

| Path | Task | Responsibility |
|---|---|---|
| `docs/protocol/transport.md` | 1, 8, 9, 10 | The transport write-up (spec §10 item 1) |
| `docs/re/README.md` | 2 | The #18 evidence norm or REA recipe; `AurogonEngine.dll` (spike-0 1.8) |
| `crates/fenji-proto/src/{lib,error,header,codec}.rs` | 3 | Crate, errors, inner header, raw-length prefix, `StreamCodec`, `StoredCodec` |
| `crates/fenji-proto/src/{scramble,frame}.rs` | 4 | Frame keys and frames, both directions |
| `crates/fenji-proto/src/{hello,variant}.rs` | 5 | Hello build/check, key fold, the AES-128 variant |
| `crates/fenji-proto/src/{handshake,client}.rs`, `tests/handshake.rs` | 6 | Server state machine; client simulator; end-to-end tests |
| `tools/stub/src/{main,server,session,speak,events}.rs`, `tools/stub/tests/speak.rs` | 7 | `--speak`, events log, sent-bytes log |
| `scripts/spike/run-client.sh`, `scripts/spike/test-run-client.sh` | 7 | `--stub-arg` |
| `crates/fenji-proto/tests/fixtures/*.hex`, `crates/fenji-proto/tests/fixtures.rs` | 8, 10 | Scrubbed real-client fixtures |
| `docs/protocol/opcodes.md` | 9, 10 | Opcode ledger with wire values |
| `crates/fenji-proto/src/{opcode,packet,…}.rs` | 10 | Defined by Task 9's amendment |
| `rust-toolchain.toml`, `scripts/versions.env`, `scripts/bootstrap.sh`, `CLAUDE.md`, `docs/development.md` | 11 | Windows target and MinGW |
| `tools/oodle-helper/` | 11 | The helper (Windows) |
| `crates/fenji-data/src/oodle_spool.rs` | 11 | `OodleSpool` decompressor |
| `scripts/spike/run-oodle-helper.sh` | 11 | Runs the helper in the sandbox |
| `crates/fenji-data/src/index.rs`, `tools/archive/src/main.rs` | 12 | `_index` lines; `archive paths` |
| `docs/re/m2-tables.md`, table code per Task 13's amendment | 13 | `.tab` format, `archive dump` |
| `docs/re/m2-summary.md`, spec notes | 14 | Records |

## Order

The critical path is Task 1 → 3 → 4 → 5 → 6 → 7 → 8 → 10.
- Tasks 2 and 11 start right after Task 1 and run alongside the critical path.
- Task 9's Steps 2, 3 and 5 start after Task 1. Its Step 4 needs R1's auth capture (Task 8, Step 2).
- Task 9 must finish before Task 10.
- Task 12 follows Task 11, and Task 13 follows Task 12.
- If R3 (Task 8) fails, Task 10 waits on gate G2, while Tasks 9 and 11–13 go on.
- Task 14 closes M2.

---

### Task 0: M2 tracking

**Files:** none in git (GitHub and the local ledger).

- [ ] **Step 1: Merge the spec and plan PR.** Push `docs/m2-transport-design` (the spec commit and this plan's commit) and open one PR into `dev`: `docs: add the M2 transport design spec and implementation plan`. Wait for CI and squash-merge.

- [ ] **Step 2: Record decision P2 on gate #39 and on #51:**

```bash
gh issue comment 39 --body "Decided at M2 planning (owner, 2026-10-12): route (c), the client's own oo2core_6_win64.dll called from a helper inside the sandbox, for dev analysis on the owner's machines only. Never on the VPS, in the launcher or for players; server-side use would be a new gate. Spec: docs/superpowers/specs/2026-10-12-m2-transport-design.md §2 (P2), §6."
gh issue comment 51 --body "Route decided at M2 planning: (c), see #39 and the M2 spec §6. Implemented by M2 plan Tasks 11–13."
```

- [ ] **Step 3: Create one `Task` issue per task (1–14)** in milestone `M2 Transport + handshake` (title and labels checked at planning).

```bash
M="M2 Transport + handshake"
for t in "1|Transport write-up: hello, variants, keys, frames, Oodle wrapper, auth|type:re,area:docs" \
         "2|Evidence norm for gujianol.exe (#18) and AurogonEngine.dll exports (spike-0 1.8)|type:re,area:docs" \
         "3|fenji-proto: crate, inner header, raw-length prefix, StoredCodec|type:task,area:proto" \
         "4|fenji-proto: frame keys and frames, both directions|type:task,area:proto" \
         "5|fenji-proto: hello and the AES-128 variant|type:task,area:proto" \
         "6|fenji-proto: server handshake and client simulator|type:task,area:proto" \
         "7|fenji-stub --speak, events log, run-client.sh --stub-arg|type:task,area:tools" \
         "8|Rungs R1-R3: hello, auth fixture, the stored-frame Oodle test|type:re,area:docs" \
         "9|Post-auth reading: QUEUE to WORK, opcodes, payload encoding, time sync|type:re,area:docs" \
         "10|First packets: opcode and packet decoding, rungs R4-R5|type:task,area:proto" \
         "11|Oodle helper, MinGW pins, OodleSpool|type:task,area:data" \
         "12|_index decoded and archive paths|type:task,area:data" \
         "13|Table format and archive dump|type:task,area:data" \
         "14|M2 close-out: records, spec notes, final review, v0.3.0|type:task,area:docs"; do
  IFS='|' read -r n title labels <<<"$t"
  gh issue create --milestone "$M" --title "$title" \
    $(printf -- '--label %s ' ${labels//,/ }) \
    --body "### Goal
${title}.

### Plan reference
docs/superpowers/plans/2026-10-12-m2-transport.md, Task ${n}

### Done when
The task's steps in the plan are complete and its PR is merged into dev."
done
```

#51 is closed by Task 13's PR, and #18 by Task 2's.

- [ ] **Step 4: Start the ledger** `re/m2/progress.md` (git-ignored): a table `Task N → #M`, then a line per task as it completes, as in M1's ledger. Branch names below use `<issue>` for the task's own issue number.

---

### Task 1: Transport write-up

**Files:**
- Create: `docs/protocol/transport.md`
- Modify: `docs/protocol/README.md` (the `transport.md` row now links), `docs/README.md` (one row)
- Local only: `re/m2/transport/`

**Interfaces:**
- Consumes the planning findings F1–F8.
- Produces, for Tasks 3–6, the verified constants:
  - `K = 0xE62F7B30`;
  - the hello check;
  - case 3 = AES-128 with tail constant 0xC6;
  - the key fold;
  - the key steps 0xD736 (server → client), 0x28CA and 0xB4FB (client → server);
  - the raw-length prefix forms;
  - the auth layout;
  - the `ok` rule;
  - the header layout.

  If any of them differs from F1–F8, this task's PR also corrects the planning findings in this plan (a one-line "corrected by Task 1" note per item). Tasks 3–6 use the corrected values.

This is RE work: a clean session or agent only. Read-only Ghidra (no renames, comments, scripts or `debugger_*` tools).

- [ ] **Step 1: Branch.** `git switch dev && git pull --ff-only && git switch -c docs/re-<issue>-transport && mkdir -p re/m2/transport`

- [ ] **Step 2: Re-verify F1–F8 in Ghidra.** For each finding, read the cited function with `force_decompile`, and where a constant matters, check the listing with `disassemble_bytes` (`dry_run: true`). Note each check in `re/m2/transport/checks.md`: finding, address, what you saw, and agrees/differs. In particular:
  - **F3:** confirm the register added at 0x14029C6C0 (`LEA EAX,[RDX+RBP]`) holds 0x28CA. Find where RBP is set in FUN_14029C600.
  - **F2:** the AES family's tail constant in both transforms (0xC6). Confirm that case 3's init passes 16 to both the fold and the setup.
  - **F2:** case 19's init (FUN_140299A10): its key length.
  - **F1:** the 0x24924925 division is `w1 / 14` (check the shifts at the hello check), so the index is `(w1 % 14) + 2`.
  - **F5:** what object DAT_1431FC8E0 points to, and what fills its first 16 bytes. Follow `get_xrefs_to` on 0x1431FC8E0; spike-0 4.25 calls it the shared-dictionary object. Record it as Inferred, or Unknown.
  - **F4:** what the receive loop does with a decoded length of 0, and with the `FF` prefix.
  - The error codes 0x5F5E101–0x5F5E106: where each is set, and the state it leaves.
  - **Gate G1:** if the re-read shows that the hello's check depends on anything a server of ours can't produce (a key not in the client, a signature), stop all handshake work. Open a `Decision gate` issue with the evidence and tell the owner. Tasks 2 and 11–13 go on.

- [ ] **Step 3: Read `ks.pickle_pack`'s registration**, only to find it (its format is Task 9's). `search_strings` for `pickle_pack`, then `get_xrefs_to`, and record the native function's address for Task 9.

- [ ] **Step 4: Write `docs/protocol/transport.md`.** Prose and tables only, every row tagged with its evidence. Sections:
  - `# Transport`: a summary of three or four sentences, and a Method line naming the pinned exe's SHA-256 and saying that nothing was executed.
  - `## 1. Connection and states`: the `KS_NET_CLIENT_ST_*` values 0–7 (spike-0 4.10–4.11), what each poll does in each state (FUN_14029ADD0), and the error codes.
  - `## 2. The hello`: the 64 bytes, the check, `K`, and which words choose the variants.
  - `## 3. Variants`: the case → family → cipher table (F2), the init / send / receive slots, the key fold, the AES transforms and the tail XOR, and which side encrypts. The other families' differences are listed (cipher, key length where read); they aren't implemented.
  - `## 4. Frames and keys`: both directions, the time-sync frame, the 0x800 send chunk, the initial keys and the steps.
  - `## 5. The Oodle wrapper`: the raw-length prefix forms and the decode call. Server frames always pass the decoder (F4). The stored-frame test belongs to R3 (Task 8).
  - `## 6. Auth`: the client's message layout (F5), the 6-byte forms, the `ok` rule, the close reason.
  - `## 7. Inner messages`: the header (F6), and what Task 9 adds (dispatch, opcodes).
  - `## 8. Open questions`: `QUEUE` → `WORK`; payload encoding; whether the server must send time syncs; the R3 result.
  - `## Evidence index`: key → address or local file under `re/m2/`.

- [ ] **Step 5: Update the indexes.** In `docs/protocol/README.md`, link the `transport.md` row. Add to `docs/README.md`:

```markdown
| [`protocol/transport.md`](protocol/transport.md) | M2: the client's transport: hello, variants, frames and keys, the Oodle wrapper, auth. |
```

- [ ] **Step 6: Commit and merge.**

```bash
scripts/check-forbidden-files.sh
git add docs/protocol/transport.md docs/protocol/README.md docs/README.md docs/superpowers/plans/2026-10-12-m2-transport.md
git commit -m "docs(re): write up the client's transport and handshake" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 2: Evidence norm for `gujianol.exe` (#18) and `AurogonEngine.dll` (spike-0 1.8)

**Files:**
- Modify: `docs/re/README.md` (a section "Evidence for `gujianol.exe`"), `docs/re/spike-0.md` (1.8's row: an "M2 answer" note)
- Local only: `re/m2/rea/`

**Interfaces:** produces the citation norm every later RE task follows: REA Evidence IDs through the recipe, or pinned-exe addresses as the norm.

This is RE work (clean session only). Close every REA session you open.

- [ ] **Step 1: Branch.** `git switch dev && git pull --ff-only && git switch -c docs/re-<issue>-evidence-norm && mkdir -p re/m2/rea`

- [ ] **Step 2: Try a targeted REA recipe** on `$FENJI_CLIENT_PATH/bin64/gujianol.exe`. Do not run a whole-binary analysis.
  1. `open_binary` in the lightest mode it offers.
  2. `analyze_function` on FUN_14029C330 (0x14029C330).
  3. `procedure_pseudo_code` on the same address.
  4. `close_binary`.

  When a tool result is written to an overflow file, read that file with Read. Record in `re/m2/rea/attempt.md`: each call, its time, whether it returned an Evidence ID, and the ID.

- [ ] **Step 3: Decide the norm.**
  - If Step 2 produced Evidence IDs within about 5 minutes per function, the recipe is the norm for new findings, and Ghidra addresses stay acceptable alongside.
  - Otherwise, pinned-exe addresses from the shared Ghidra project are the norm for `gujianol.exe`, and REA Evidence IDs are used for smaller binaries.

- [ ] **Step 4: `AurogonEngine.dll`.**
  - With REA (the DLL is small enough), list its named exports.
  - For three of them that look like network or engine functions, find the matching function in `gujianol.exe`. Use Ghidra's `find_similar_functions_fuzzy`, or the string references the DLL's function shares with the exe.
  - Record whether the exports name exe functions (yes / partly / no) and the three examples.

- [ ] **Step 5: Write it up.**
  - `docs/re/README.md` gets the section "Evidence for `gujianol.exe`": the norm, the recipe or the reason there isn't one, and how to cite (`FUN_<addr>` or `ev_<id>`).
  - `docs/re/spike-0.md` 1.8 gets the note `**M2 answer:** <yes/partly/no>, see docs/re/README.md`.

- [ ] **Step 6: Commit and merge.**

```bash
git add docs/re/README.md docs/re/spike-0.md
git commit -m "docs(re): settle the evidence norm for gujianol.exe and read AurogonEngine's exports" -m "Closes #<issue>
Closes #18" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 3: `fenji-proto`: crate, inner header, raw-length prefix, `StoredCodec`

**Files:**
- Create: `crates/fenji-proto/Cargo.toml`, `crates/fenji-proto/src/lib.rs`, `src/error.rs`, `src/header.rs`, `src/codec.rs`
- Modify: root `Cargo.toml` (member, workspace dependency)

**Interfaces:**
- Produces:

```rust
pub enum fenji_proto::Error { BodyTooLong(usize), FrameLength(usize), BadRawLength(&'static str),
    NotStored { raw: usize, got: usize }, BadHello(String), Unsupported(u32), BadAuth(String),
    WrongState { action: &'static str, state: &'static str } }
pub type fenji_proto::Result<T>;
pub const header::HEADER_LEN: usize = 4;
pub const header::MAX_BODY: usize = 0xFF_FFFF;
pub struct header::Header { pub ty: u8, pub len: usize }
impl Header { pub fn new(ty: u8, len: usize) -> Result<Self>; pub fn to_bytes(self) -> [u8; 4];
              pub fn from_bytes(b: [u8; 4]) -> Self }
pub fn header::write_message(ty: u8, body: &[u8], out: &mut Vec<u8>) -> Result<()>;
pub fn header::split_message(buf: &[u8]) -> Option<(Header, &[u8], usize)>;
pub fn codec::write_raw_len(n: usize, out: &mut Vec<u8>) -> Result<()>;
pub fn codec::read_raw_len(buf: &[u8]) -> Result<(usize, usize)>;  // (raw length, prefix length)
pub trait codec::StreamCodec { fn encode(&self, raw: &[u8], out: &mut Vec<u8>) -> Result<()>;
                               fn decode(&self, payload: &[u8]) -> Result<Vec<u8>>; }
pub struct codec::StoredCodec;
```

- [ ] **Step 1: Branch and create the crate.** `git switch dev && git pull --ff-only && git switch -c feat/proto-<issue>-header-codec`

In the root `Cargo.toml`, add `"crates/fenji-proto"` to `members`, and to `[workspace.dependencies]`:

```toml
fenji-proto = { path = "crates/fenji-proto" }
```

`crates/fenji-proto/Cargo.toml`:

```toml
[package]
name = "fenji-proto"
description = "The SoLO client's transport without I/O: hello, variants, frame keys, frames, the Oodle wrapper's prefix, inner headers and the gate handshake"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
thiserror.workspace = true

[lints]
workspace = true
```

`src/lib.rs`:

```rust
//! The SoLO client's transport, without I/O (M2 spec §4; docs/protocol/transport.md). Bytes go in;
//! bytes to send and events come out. Nothing here panics on peer input.
pub mod codec;
mod error;
pub mod header;

pub use error::{Error, Result};
```

`src/error.rs`:

```rust
/// What can go wrong reading or writing the transport.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("a message body of {0} bytes is over the limit")]
    BodyTooLong(usize),
    #[error("a frame body of {0} bytes doesn't fit a u16 length of 1..=65535")]
    FrameLength(usize),
    #[error("raw-length prefix: {0}")]
    BadRawLength(&'static str),
    #[error("stored packet: the prefix says {raw} bytes, but {got} follow")]
    NotStored { raw: usize, got: usize },
    #[error("hello: {0}")]
    BadHello(String),
    #[error("variant {0} is not implemented (docs/protocol/transport.md §3)")]
    Unsupported(u32),
    #[error("auth message: {0}")]
    BadAuth(String),
    #[error("can't {action} in state {state}")]
    WrongState {
        action: &'static str,
        state: &'static str,
    },
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 2: Write the failing header tests** at the bottom of `src/header.rs`. Create the file holding only the test module plus `use crate::{Error, Result};`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_reply_bytes() {
        // F5/F6: a 2-byte body "ok" of type 0: [0, 0 ^ (2 >> 1), 2, 0].
        let mut out = Vec::new();
        write_message(0, b"ok", &mut out).unwrap();
        assert_eq!(out, [0x00, 0x01, 0x02, 0x00, b'o', b'k']);
    }

    #[test]
    fn auth_header_for_a_100_byte_payload() {
        // F5: length 22 + 100 = 122 = 0x7a; byte 1 = 0 ^ (122 >> 1) = 0x3d.
        assert_eq!(Header::new(0, 122).unwrap().to_bytes(), [0x00, 0x3d, 0x7a, 0x00]);
    }

    #[test]
    fn round_trips_lengths_and_types() {
        for len in [0, 1, 2, 255, 256, 0x7ff, 0x800, 65_535, 65_536, MAX_BODY] {
            for ty in [0u8, 1, 0x7f, 0x80, 0xfe, 0xff] {
                let h = Header::new(ty, len).unwrap();
                assert_eq!(Header::from_bytes(h.to_bytes()), h, "ty {ty} len {len}");
            }
        }
    }

    #[test]
    fn length_over_24_bits_is_refused() {
        assert_eq!(Header::new(0, MAX_BODY + 1), Err(Error::BodyTooLong(MAX_BODY + 1)));
    }

    #[test]
    fn split_waits_for_a_whole_message() {
        let mut buf = Vec::new();
        write_message(7, b"abc", &mut buf).unwrap();
        write_message(8, b"", &mut buf).unwrap();
        for cut in 0..7 {
            assert!(split_message(&buf[..cut]).is_none(), "cut {cut}");
        }
        let (h, body, used) = split_message(&buf).unwrap();
        assert_eq!((h.ty, body, used), (7, &b"abc"[..], 7));
        let (h, body, used) = split_message(&buf[used..]).unwrap();
        assert_eq!((h.ty, body, used), (8, &b""[..], 4));
    }
}
```

Add `pub mod header;` (already in lib.rs). Run: `cargo test -p fenji-proto header` → expected FAIL: cannot find `write_message`, `Header`, `MAX_BODY` and `split_message`.

- [ ] **Step 3: Implement `header.rs`** above the tests:

```rust
//! The 4-byte inner message header (spike-0 4.29; written by FUN_14029C330, read by FUN_14029ADD0;
//! docs/protocol/transport.md §7). `len` counts the body only.
use crate::{Error, Result};

/// Bytes in a header.
pub const HEADER_LEN: usize = 4;
/// The largest body a 24-bit length can carry.
pub const MAX_BODY: usize = 0xFF_FFFF;

/// A message's type and body length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub ty: u8,
    pub len: usize,
}

impl Header {
    pub fn new(ty: u8, len: usize) -> Result<Self> {
        if len > MAX_BODY {
            return Err(Error::BodyTooLong(len));
        }
        Ok(Self { ty, len })
    }

    /// `[len >> 16, ty ^ low byte of (len >> 1), len, len >> 8]`.
    pub fn to_bytes(self) -> [u8; HEADER_LEN] {
        let l = self.len as u32;
        [(l >> 16) as u8, self.ty ^ (l >> 1) as u8, l as u8, (l >> 8) as u8]
    }

    pub fn from_bytes(b: [u8; HEADER_LEN]) -> Self {
        let len = (u32::from(b[0]) << 16) | (u32::from(b[3]) << 8) | u32::from(b[2]);
        Self {
            ty: b[1] ^ (len >> 1) as u8,
            len: len as usize,
        }
    }
}

/// Appends one message, header then body, to `out`.
pub fn write_message(ty: u8, body: &[u8], out: &mut Vec<u8>) -> Result<()> {
    out.extend_from_slice(&Header::new(ty, body.len())?.to_bytes());
    out.extend_from_slice(body);
    Ok(())
}

/// The first whole message in `buf`: its header, its body and the bytes it takes. `None` until all
/// of it is there.
pub fn split_message(buf: &[u8]) -> Option<(Header, &[u8], usize)> {
    let h = Header::from_bytes(buf.get(..HEADER_LEN)?.try_into().ok()?);
    let body = buf.get(HEADER_LEN..HEADER_LEN + h.len)?;
    Some((h, body, HEADER_LEN + h.len))
}
```

Run: `cargo test -p fenji-proto header` → expected PASS, 5 tests.

- [ ] **Step 4: Write the failing codec tests** in `src/codec.rs` (test module only, plus `use crate::{Error, Result};`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        write_raw_len(n, &mut out).unwrap();
        out
    }

    #[test]
    fn prefix_forms() {
        // F4: < 0xFC in one byte; FC + u16; FD + 24 bits; FE + u32, all little-endian.
        assert_eq!(prefix(0), [0x00]);
        assert_eq!(prefix(0xfb), [0xfb]);
        assert_eq!(prefix(0xfc), [0xfc, 0xfc, 0x00]);
        assert_eq!(prefix(0x1234), [0xfc, 0x34, 0x12]);
        assert_eq!(prefix(0x1_0000), [0xfd, 0x00, 0x00, 0x01]);
        assert_eq!(prefix(0xff_ffff), [0xfd, 0xff, 0xff, 0xff]);
        assert_eq!(prefix(0x100_0000), [0xfe, 0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn prefix_round_trips() {
        for n in [0, 1, 0xfb, 0xfc, 0xffff, 0x1_0000, 0xff_ffff, 0x100_0000, u32::MAX as usize] {
            let p = prefix(n);
            assert_eq!(read_raw_len(&p), Ok((n, p.len())), "{n:#x}");
        }
    }

    #[test]
    fn raw_len_ff_is_refused() {
        assert!(matches!(read_raw_len(&[0xff, 1, 2, 3, 4]), Err(Error::BadRawLength(_))));
    }

    #[test]
    fn truncated_prefix_is_an_error() {
        for bad in [&[][..], &[0xfc, 1], &[0xfd, 1, 2], &[0xfe, 1, 2, 3]] {
            assert!(matches!(read_raw_len(bad), Err(Error::BadRawLength(_))), "{bad:02x?}");
        }
    }

    #[test]
    fn stored_codec_round_trips_and_checks_length() {
        let mut out = Vec::new();
        StoredCodec.encode(b"\x00\x01\x02\x00ok", &mut out).unwrap();
        assert_eq!(out, [0x06, 0x00, 0x01, 0x02, 0x00, b'o', b'k']);
        assert_eq!(StoredCodec.decode(&out).unwrap(), b"\x00\x01\x02\x00ok");
        assert_eq!(
            StoredCodec.decode(&[0x05, 1, 2]),
            Err(Error::NotStored { raw: 5, got: 2 })
        );
    }
}
```

Run: `cargo test -p fenji-proto codec` → expected FAIL (missing items).

- [ ] **Step 5: Implement `codec.rs`:**

```rust
//! The per-frame Oodle wrapper (FUN_14029B0A0; docs/protocol/transport.md §5): every data frame the
//! server sends carries a raw-length prefix, then one packet for `OodleNetwork1TCP_Decode`. The
//! client has no encoder, so the server's codec decides what that packet is.
use crate::{Error, Result};

/// Appends `n` in the shortest prefix form.
pub fn write_raw_len(n: usize, out: &mut Vec<u8>) -> Result<()> {
    match n {
        0..=0xfb => out.push(n as u8),
        0xfc..=0xffff => {
            out.push(0xfc);
            out.extend_from_slice(&(n as u16).to_le_bytes());
        }
        0x1_0000..=0xff_ffff => {
            out.push(0xfd);
            out.extend_from_slice(&(n as u32).to_le_bytes()[..3]);
        }
        _ => {
            let n = u32::try_from(n).map_err(|_| Error::BadRawLength("over 32 bits"))?;
            out.push(0xfe);
            out.extend_from_slice(&n.to_le_bytes());
        }
    }
    Ok(())
}

/// Reads a prefix: `(raw length, prefix length)`. `FF` is refused: the client reads it as no
/// prefix and a raw length of 0, which no packet of ours uses.
pub fn read_raw_len(buf: &[u8]) -> Result<(usize, usize)> {
    let take = |n: usize| buf.get(1..1 + n).ok_or(Error::BadRawLength("truncated"));
    match *buf.first().ok_or(Error::BadRawLength("truncated"))? {
        n @ 0..=0xfb => Ok((usize::from(n), 1)),
        0xfc => {
            let b = take(2)?;
            Ok((usize::from(u16::from_le_bytes([b[0], b[1]])), 3))
        }
        0xfd => {
            let b = take(3)?;
            Ok((u32::from_le_bytes([b[0], b[1], b[2], 0]) as usize, 4))
        }
        0xfe => {
            let b = take(4)?;
            Ok((u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize, 5))
        }
        0xff => Err(Error::BadRawLength("0xff is not a prefix")),
    }
}

/// What a frame's payload holds after the prefix.
pub trait StreamCodec {
    /// Appends the payload for `raw`: the prefix, then the packet.
    fn encode(&self, raw: &[u8], out: &mut Vec<u8>) -> Result<()>;
    /// The raw bytes of one payload.
    fn decode(&self, payload: &[u8]) -> Result<Vec<u8>>;
}

/// The raw bytes themselves, with a raw length equal to the packet's length. Whether the client's
/// decoder accepts this is rung R3's question (M2 spec §2, P1).
#[derive(Debug, Default, Clone, Copy)]
pub struct StoredCodec;

impl StreamCodec for StoredCodec {
    fn encode(&self, raw: &[u8], out: &mut Vec<u8>) -> Result<()> {
        write_raw_len(raw.len(), out)?;
        out.extend_from_slice(raw);
        Ok(())
    }

    fn decode(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let (raw, used) = read_raw_len(payload)?;
        let rest = &payload[used..];
        if rest.len() != raw {
            return Err(Error::NotStored {
                raw,
                got: rest.len(),
            });
        }
        Ok(rest.to_vec())
    }
}
```

Run: `cargo test -p fenji-proto` → expected PASS, 10 tests.

- [ ] **Step 6: Gates and commit.**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo deny check
git add Cargo.toml Cargo.lock crates/fenji-proto
git commit -m "feat(proto): add fenji-proto with the inner header and the stored codec" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 4: `fenji-proto`: frame keys and frames, both directions

**Files:**
- Create: `crates/fenji-proto/src/scramble.rs`, `crates/fenji-proto/src/frame.rs`
- Modify: `crates/fenji-proto/src/lib.rs`

**Interfaces:**
- Consumes `Error::FrameLength`.
- Produces:

```rust
pub struct scramble::DownKey(pub u16);  impl DownKey { pub fn step(&mut self) }
pub struct scramble::UpKey(pub u16);    impl UpKey { pub fn step_data(&mut self); pub fn step_time_sync(&mut self) }
pub const frame::MAX_FRAME_BODY: usize = 0xFFFF;
pub const frame::SEND_CHUNK: usize = 0x800;
pub enum frame::Frame { TimeSync(u64), Data(Vec<u8>) }
pub struct frame::DownWriter;  // server side: new(), data(&mut self, body: &[u8], out: &mut Vec<u8>) -> Result<()>, time_sync(&mut self, millis: u64, out: &mut Vec<u8>)
pub struct frame::DownReader;  // client side: new(), read(&mut self, buf: &[u8]) -> Option<(Frame, usize)>
pub struct frame::UpWriter;    // client side: new(), data(..) -> Result<()>, time_sync(..)
pub struct frame::UpReader;    // server side: new(), read(..) -> Option<(Frame, usize)>
```

- [ ] **Step 1: Branch.** `git switch dev && git pull --ff-only && git switch -c feat/proto-<issue>-frames`

- [ ] **Step 2: `scramble.rs`, with its tests first.** Write the file as below, then run `cargo test -p fenji-proto scramble`. The test expectations are worked by hand from F3, so if the code is wrong they fail.

```rust
//! The frames' 16-bit length keys (spike-0 4.30; docs/protocol/transport.md §4). Both live in the
//! variant contexts that FUN_14029BA10 clears, so both are 0 once the hello is handled.

/// Server to client: the client's receive key (ctx+0x1480). It steps by 0xD736 after every frame,
/// data or time sync (FUN_14029B850; 0x14029B904, 0x14029B964).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DownKey(pub u16);

impl DownKey {
    pub fn step(&mut self) {
        self.0 = self.0.wrapping_add(0xd736);
    }
}

/// Client to server: the client's send key (ctx+0x2548). A data frame steps it to
/// `(k + 0x28CA) ^ k` (FUN_14029C600), a time-sync frame to `(k + 0xB4FB) ^ k` (FUN_14029A5B0).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UpKey(pub u16);

impl UpKey {
    pub fn step_data(&mut self) {
        self.0 = self.0.wrapping_add(0x28ca) ^ self.0;
    }

    pub fn step_time_sync(&mut self) {
        self.0 = self.0.wrapping_add(0xb4fb) ^ self.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_key_sequence() {
        let mut k = DownKey::default();
        let mut seen = Vec::new();
        for _ in 0..3 {
            seen.push(k.0);
            k.step();
        }
        // 0, 0xD736, 0xD736 * 2 mod 2^16 = 0xAE6C.
        assert_eq!(seen, [0x0000, 0xd736, 0xae6c]);
    }

    #[test]
    fn up_key_sequences() {
        let mut k = UpKey::default();
        k.step_data(); // (0 + 0x28CA) ^ 0 = 0x28CA
        assert_eq!(k.0, 0x28ca);
        k.step_data(); // (0x28CA + 0x28CA) ^ 0x28CA = 0x5194 ^ 0x28CA = 0x795E
        assert_eq!(k.0, 0x795e);
        k.step_time_sync(); // (0x795E + 0xB4FB) ^ 0x795E = 0x2E59 ^ 0x795E = 0x5707
        assert_eq!(k.0, 0x5707);
    }
}
```

Expected: PASS, 2 tests. Add `pub mod scramble;` to `lib.rs`.

- [ ] **Step 3: Write the failing frame tests** in `src/frame.rs` (test module, plus the `use` lines of Step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_down_frames_bytes() {
        let mut w = DownWriter::new();
        let mut out = Vec::new();
        w.data(b"abc", &mut out).unwrap(); // key 0: length 3 as is
        w.data(b"x", &mut out).unwrap(); // key 0xD736: 1 ^ 0xD736 = 0xD737
        assert_eq!(out, [0x03, 0x00, b'a', b'b', b'c', 0x37, 0xd7, b'x']);
    }

    #[test]
    fn down_round_trip_with_time_sync() {
        let mut w = DownWriter::new();
        let mut out = Vec::new();
        w.data(b"one", &mut out).unwrap();
        w.time_sync(0x0123_4567_89ab_cdef, &mut out);
        w.data(&[7; 0x800], &mut out).unwrap();
        let mut r = DownReader::new();
        let mut at = 0;
        let mut frames = Vec::new();
        while let Some((f, used)) = r.read(&out[at..]) {
            frames.push(f);
            at += used;
        }
        assert_eq!(at, out.len());
        assert_eq!(
            frames,
            [
                Frame::Data(b"one".to_vec()),
                Frame::TimeSync(0x0123_4567_89ab_cdef),
                Frame::Data(vec![7; 0x800]),
            ]
        );
    }

    #[test]
    fn up_round_trip_with_time_sync() {
        let mut w = UpWriter::new();
        let mut out = Vec::new();
        w.time_sync(42, &mut out);
        w.data(b"hello", &mut out).unwrap();
        w.time_sync(43, &mut out);
        let mut r = UpReader::new();
        let mut at = 0;
        let mut frames = Vec::new();
        while let Some((f, used)) = r.read(&out[at..]) {
            frames.push(f);
            at += used;
        }
        assert_eq!(
            frames,
            [Frame::TimeSync(42), Frame::Data(b"hello".to_vec()), Frame::TimeSync(43)]
        );
    }

    #[test]
    fn up_time_sync_bytes() {
        // Key 0: the length word is the key itself (0), then the clock's four words.
        let mut out = Vec::new();
        UpWriter::new().time_sync(0x0004_0003_0002_0001, &mut out);
        assert_eq!(out, [0, 0, 1, 0, 2, 0, 3, 0, 4, 0]);
    }

    #[test]
    fn partial_frames_wait_and_keep_the_key() {
        let mut w = UpWriter::new();
        let mut out = Vec::new();
        w.data(b"first", &mut out).unwrap();
        w.data(b"second", &mut out).unwrap();
        let mut r = UpReader::new();
        for cut in 0..7 {
            assert!(r.read(&out[..cut]).is_none(), "cut {cut}");
        }
        let (f, used) = r.read(&out).unwrap();
        assert_eq!(f, Frame::Data(b"first".to_vec()));
        let (f, _) = r.read(&out[used..]).unwrap();
        assert_eq!(f, Frame::Data(b"second".to_vec()));
    }

    #[test]
    fn frame_body_limits() {
        let mut out = Vec::new();
        assert_eq!(DownWriter::new().data(b"", &mut out), Err(Error::FrameLength(0)));
        let big = vec![0; MAX_FRAME_BODY + 1];
        assert_eq!(
            UpWriter::new().data(&big, &mut out),
            Err(Error::FrameLength(MAX_FRAME_BODY + 1))
        );
        assert!(out.is_empty());
    }
}
```

Run: `cargo test -p fenji-proto frame` → expected FAIL (missing items).

- [ ] **Step 4: Implement `frame.rs`** above the tests, and add `pub mod frame;` to `lib.rs`:

```rust
//! Frames (FUN_14029B850 receives, FUN_14029C600 and FUN_14029A5B0 send;
//! docs/protocol/transport.md §4). A frame is a `u16` LE length XOR the direction's key, then the
//! body. A length of 0 is a time-sync frame: four more `u16`s, the 64-bit millisecond clock low word
//! first, each XOR the key. Bodies are opaque here: the variant transform and the Oodle wrapper are
//! the caller's.
use crate::scramble::{DownKey, UpKey};
use crate::{Error, Result};

/// The most a `u16` length can say.
pub const MAX_FRAME_BODY: usize = 0xffff;
/// The client sends data frames of at most this many bytes, and so does our server.
pub const SEND_CHUNK: usize = 0x800;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    TimeSync(u64),
    Data(Vec<u8>),
}

fn check_body(body: &[u8]) -> Result<()> {
    if body.is_empty() || body.len() > MAX_FRAME_BODY {
        return Err(Error::FrameLength(body.len()));
    }
    Ok(())
}

fn put_time(millis: u64, key: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(&key.to_le_bytes()); // a length of 0, XOR the key
    for i in 0..4 {
        out.extend_from_slice(&(((millis >> (16 * i)) as u16) ^ key).to_le_bytes());
    }
}

/// Reads one frame from `buf` with `key`, without stepping it: `(frame, bytes used)`.
fn take(buf: &[u8], key: u16) -> Option<(Frame, usize)> {
    let word = |i: usize| -> Option<u16> {
        Some(u16::from_le_bytes(buf.get(2 * i..2 * i + 2)?.try_into().ok()?) ^ key)
    };
    let len = usize::from(word(0)?);
    if len == 0 {
        let mut millis = 0u64;
        for i in 0..4 {
            millis |= u64::from(word(1 + i)?) << (16 * i);
        }
        return Some((Frame::TimeSync(millis), 10));
    }
    let body = buf.get(2..2 + len)?;
    Some((Frame::Data(body.to_vec()), 2 + len))
}

/// Server side: writes frames to the client.
#[derive(Debug, Default)]
pub struct DownWriter {
    key: DownKey,
}

impl DownWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn data(&mut self, body: &[u8], out: &mut Vec<u8>) -> Result<()> {
        check_body(body)?;
        out.extend_from_slice(&(body.len() as u16 ^ self.key.0).to_le_bytes());
        out.extend_from_slice(body);
        self.key.step();
        Ok(())
    }

    pub fn time_sync(&mut self, millis: u64, out: &mut Vec<u8>) {
        put_time(millis, self.key.0, out);
        self.key.step();
    }
}

/// Client side: reads the server's frames. `None` means more bytes are needed; the key moves only
/// when a whole frame is read.
#[derive(Debug, Default)]
pub struct DownReader {
    key: DownKey,
}

impl DownReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&mut self, buf: &[u8]) -> Option<(Frame, usize)> {
        let got = take(buf, self.key.0)?;
        self.key.step();
        Some(got)
    }
}

/// Client side: writes frames to the server.
#[derive(Debug, Default)]
pub struct UpWriter {
    key: UpKey,
}

impl UpWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn data(&mut self, body: &[u8], out: &mut Vec<u8>) -> Result<()> {
        check_body(body)?;
        out.extend_from_slice(&(body.len() as u16 ^ self.key.0).to_le_bytes());
        out.extend_from_slice(body);
        self.key.step_data();
        Ok(())
    }

    pub fn time_sync(&mut self, millis: u64, out: &mut Vec<u8>) {
        put_time(millis, self.key.0, out);
        self.key.step_time_sync();
    }
}

/// Server side: reads the client's frames.
#[derive(Debug, Default)]
pub struct UpReader {
    key: UpKey,
}

impl UpReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&mut self, buf: &[u8]) -> Option<(Frame, usize)> {
        let got = take(buf, self.key.0)?;
        match got.0 {
            Frame::TimeSync(_) => self.key.step_time_sync(),
            Frame::Data(_) => self.key.step_data(),
        }
        Some(got)
    }
}
```

Run: `cargo test -p fenji-proto` → expected PASS (10 + 2 + 6 = 18 tests).

- [ ] **Step 5: Gates and commit.**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked
git add crates/fenji-proto
git commit -m "feat(proto): add frame keys and frames for both directions" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 5: `fenji-proto`: hello and the AES-128 variant

**Files:**
- Create: `crates/fenji-proto/src/hello.rs`, `crates/fenji-proto/src/variant.rs`
- Modify: `crates/fenji-proto/src/lib.rs`, `crates/fenji-proto/Cargo.toml`, root `Cargo.toml` (`aes = "0.9.3"`)
- Local only: `re/m2/vectors/`

**Interfaces:**
- Consumes `Error::{BadHello, Unsupported}`.
- Produces:

```rust
pub const hello::HELLO_LEN: usize = 64;
pub const hello::CLIENT_MAGIC: u32 = 0xE62F_7B30;
pub struct hello::Variants { pub down: u32, pub up: u32 }   // down = w0 % 27 (server→client), up = w1 % 27
pub fn hello::build(fill: [u8; 64], v: Variants, magic: u32) -> Result<[u8; 64]>;
pub fn hello::check(hello: &[u8; 64], magic: u32) -> Result<Variants>;
pub const variant::VARIANT_COUNT: u32 = 27;
pub const variant::AES128: u32 = 3;
pub fn variant::fold_key(hello: &[u8; 64], n: usize) -> Vec<u8>;
pub struct variant::Cipher;   // new(variant: u32, hello: &[u8; 64]) -> Result<Self>
impl Cipher { pub fn encrypt(&self, buf: &mut [u8]); pub fn decrypt(&self, buf: &mut [u8]) }
```

- [ ] **Step 1: Branch and add the dependency.** `git switch dev && git pull --ff-only && git switch -c feat/proto-<issue>-hello-variant`. Root `[workspace.dependencies]`: `aes = "0.9.3"`. In `crates/fenji-proto/Cargo.toml` `[dependencies]`: `aes.workspace = true`.

- [ ] **Step 2: Try to get golden vectors from the client's own code** (RE step; clean session).
  - Use the `ghidra` MCP's `emulate_function` on:
    - FUN_14029BE20 with `n = 16` and a 64-byte input of bytes `0x00..=0x3f`, expecting the fold;
    - FUN_140299B90 (the AES send transform) and FUN_140299AD0 on a 21-byte buffer `0x00..=0x14`, with a context set up by FUN_140299C50 from the same 64-byte hello.
  - Save each input and output as hex in `re/m2/vectors/<function>.txt`.
  - If the emulator can't run a function (calls it can't follow, or the stack cookie), record why and go on. Steps 3–5's expectations are computed independently, and Task 8's real-client run is the golden check.
  - If emulator vectors exist, add each as a test in `variant.rs` named `client_emulated_<function>`, citing the address and the vector file.

- [ ] **Step 3: Write the failing hello tests** in `src/hello.rs` (test module, plus `use crate::{Error, Result};`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn words(h: &[u8; 64]) -> [u32; 16] {
        core::array::from_fn(|i| u32::from_le_bytes(h[4 * i..4 * i + 4].try_into().unwrap()))
    }

    #[test]
    fn built_hello_passes_the_clients_check() {
        for seed in 0..=255u8 {
            let fill = core::array::from_fn(|i| (i as u8).wrapping_mul(37) ^ seed);
            let v = Variants { down: u32::from(seed) % 27, up: (u32::from(seed) * 7) % 27 };
            let h = build(fill, v, CLIENT_MAGIC).unwrap();
            assert_eq!(check(&h, CLIENT_MAGIC), Ok(v), "seed {seed}");
        }
    }

    #[test]
    fn check_is_the_clients_rule() {
        // F1: w[(w1 % 14) + 2] == (K | 0xC0000000) ^ w1 ^ w0, with w0 = 3 and w1 = 3.
        let mut h = [0u8; 64];
        h[0..4].copy_from_slice(&3u32.to_le_bytes());
        h[4..8].copy_from_slice(&3u32.to_le_bytes());
        let idx = (3 % 14) + 2; // 5
        h[4 * idx..4 * idx + 4].copy_from_slice(&CLIENT_MAGIC.to_le_bytes()); // K ^ 3 ^ 3
        assert_eq!(check(&h, CLIENT_MAGIC), Ok(Variants { down: 3, up: 3 }));
        h[4 * idx] ^= 1;
        assert!(matches!(check(&h, CLIENT_MAGIC), Err(Error::BadHello(_))));
    }

    #[test]
    fn build_keeps_the_fill_where_the_rule_allows() {
        let fill = [0xa5; 64];
        let h = build(fill, Variants { down: 3, up: 3 }, CLIENT_MAGIC).unwrap();
        let w = words(&h);
        let idx = (w[1] % 14 + 2) as usize;
        for (i, &x) in w.iter().enumerate().skip(2) {
            if i != idx {
                assert_eq!(x, 0xa5a5_a5a5, "word {i}");
            }
        }
    }

    #[test]
    fn variant_out_of_range_is_refused() {
        let r = build([0; 64], Variants { down: 27, up: 0 }, CLIENT_MAGIC);
        assert!(matches!(r, Err(Error::BadHello(_))));
    }
}
```

Run: `cargo test -p fenji-proto hello` → expected FAIL (missing items). Add `pub mod hello;` to `lib.rs`.

- [ ] **Step 4: Implement `hello.rs`:**

```rust
//! The server's 64-byte hello (FUN_14029C330; docs/protocol/transport.md §2). The client reads it
//! as 16 little-endian words. It accepts it when `w[(w1 % 14) + 2] == (K | 0xC0000000) ^ w1 ^ w0`,
//! then picks its receive variant from `w0 % 27` and its send variant from `w1 % 27`. The hello is
//! sent in clear and the variant keys are folded from it, so it needs to be unique, not secret.
use crate::variant::VARIANT_COUNT;
use crate::{Error, Result};

pub const HELLO_LEN: usize = 64;
/// `net_client_create`'s integer argument (net_manager.luac `0.20`), kept at ctx+0x50 by
/// FUN_14029A900 (docs/protocol/login.md 2.6–2.7).
pub const CLIENT_MAGIC: u32 = 0xe62f_7b30;

/// The variant for each direction: `down` (server to client) is `w0 % 27`, `up` is `w1 % 27`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variants {
    pub down: u32,
    pub up: u32,
}

fn word(h: &[u8; HELLO_LEN], i: usize) -> u32 {
    u32::from_le_bytes([h[4 * i], h[4 * i + 1], h[4 * i + 2], h[4 * i + 3]])
}

fn set_word(h: &mut [u8; HELLO_LEN], i: usize, v: u32) {
    h[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
}

/// The largest multiple of 27 that fits, so that `seed * 27 + v` can't overflow.
const SEEDS: u32 = u32::MAX / VARIANT_COUNT;

/// Builds a hello from `fill` (random bytes) that chooses `v` and passes the client's check.
pub fn build(fill: [u8; HELLO_LEN], v: Variants, magic: u32) -> Result<[u8; HELLO_LEN]> {
    if v.down >= VARIANT_COUNT || v.up >= VARIANT_COUNT {
        return Err(Error::BadHello(format!("variants {v:?} are not all below 27")));
    }
    let mut h = fill;
    let w0 = (word(&h, 0) % SEEDS) * VARIANT_COUNT + v.down;
    let w1 = (word(&h, 1) % SEEDS) * VARIANT_COUNT + v.up;
    set_word(&mut h, 0, w0);
    set_word(&mut h, 1, w1);
    set_word(&mut h, (w1 % 14 + 2) as usize, (magic | 0xc000_0000) ^ w1 ^ w0);
    Ok(h)
}

/// The client's check: the variants if the hello passes.
pub fn check(h: &[u8; HELLO_LEN], magic: u32) -> Result<Variants> {
    let (w0, w1) = (word(h, 0), word(h, 1));
    let idx = (w1 % 14 + 2) as usize;
    if word(h, idx) != (magic | 0xc000_0000) ^ w1 ^ w0 {
        return Err(Error::BadHello(format!("word {idx} fails the check")));
    }
    Ok(Variants {
        down: w0 % VARIANT_COUNT,
        up: w1 % VARIANT_COUNT,
    })
}
```

(It needs `variant::VARIANT_COUNT`; Step 6 adds it. Until then, `cargo test` fails to compile; carry on to Step 5.)

- [ ] **Step 5: Write the failing variant tests** in `src/variant.rs` (test module, plus the `use` lines of Step 6):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{BlockCipherEncrypt, KeyInit};

    fn hello() -> [u8; 64] {
        core::array::from_fn(|i| i as u8)
    }

    #[test]
    fn fold_xors_the_hello_into_the_key() {
        // F2 (FUN_14029BE20): key[i % 16] ^= hello[i].
        let k = fold_key(&hello(), 16);
        let want: Vec<u8> = (0..16u8).map(|i| i ^ (i + 16) ^ (i + 32) ^ (i + 48)).collect();
        assert_eq!(k, want);
        // n = 24: bytes 48..64 fold onto key[0..16] as well.
        let k = fold_key(&hello(), 24);
        let want: Vec<u8> = (0..24u8)
            .map(|i| if i < 16 { i ^ (i + 24) ^ (i + 48) } else { i ^ (i + 24) })
            .collect();
        assert_eq!(k, want);
    }

    #[test]
    fn aes_blocks_are_plain_aes_128_ecb() {
        let c = Cipher::new(AES128, &hello()).unwrap();
        let key = fold_key(&hello(), 16);
        let aes = aes::Aes128::new_from_slice(&key).unwrap();
        let mut want = [0x11u8; 16];
        aes.encrypt_block((&mut want).into());
        let mut got = [0x11u8; 16];
        c.encrypt(&mut got);
        assert_eq!(got, want);
    }

    #[test]
    fn tail_bytes_xor_count_and_c6() {
        // 3 tail bytes after one block: byte j gets (3 - j) ^ 0xC6.
        let c = Cipher::new(AES128, &hello()).unwrap();
        let mut buf = [0u8; 19];
        c.encrypt(&mut buf);
        assert_eq!(&buf[16..], &[3 ^ 0xc6, 2 ^ 0xc6, 1 ^ 0xc6]);
    }

    #[test]
    fn decrypt_inverts_encrypt_for_all_lengths() {
        let c = Cipher::new(AES128, &hello()).unwrap();
        for len in 0..80 {
            let plain: Vec<u8> = (0..len).map(|i| (i * 7) as u8).collect();
            let mut buf = plain.clone();
            c.encrypt(&mut buf);
            if len > 0 {
                assert_ne!(buf, plain, "len {len}");
            }
            c.decrypt(&mut buf);
            assert_eq!(buf, plain, "len {len}");
        }
    }

    #[test]
    fn other_variants_are_unsupported() {
        for v in (0..27).filter(|&v| v != AES128) {
            assert!(matches!(Cipher::new(v, &hello()), Err(Error::Unsupported(n)) if n == v));
        }
    }
}
```

`Cipher` has no `PartialEq`, so the last test uses `matches!`. Add `pub mod variant;` to `lib.rs`. Run: `cargo test -p fenji-proto variant` → expected FAIL (missing items).

- [ ] **Step 6: Implement `variant.rs`:**

```rust
//! The stream variants (FUN_14029BA10; docs/protocol/transport.md §3). Each direction runs one of
//! 27 variants: eight LibTomCrypt block ciphers in ECB with a byte-wise tail. Only case 3, AES-128,
//! is implemented; our server always chooses it. The client encrypts what it sends (slot 2) and
//! decrypts what it receives (slot 3).
use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};

use crate::hello::HELLO_LEN;
use crate::{Error, Result};

pub const VARIANT_COUNT: u32 = 27;
/// Case 3: the hello folded to 16 bytes, then AES (init FUN_140299C50, encrypt FUN_140299B90,
/// decrypt FUN_140299AD0).
pub const AES128: u32 = 3;
/// The AES family's tail constant (FUN_140299B90, FUN_140299AD0).
const AES_TAIL: u8 = 0xc6;

/// The key fold (FUN_14029BE20): `key[i % n] ^= hello[i]` for every hello byte, from zero.
pub fn fold_key(hello: &[u8; HELLO_LEN], n: usize) -> Vec<u8> {
    let mut key = vec![0u8; n];
    for (i, b) in hello.iter().enumerate() {
        key[i % n] ^= b;
    }
    key
}

/// Byte `j` of an `n`-byte tail is XORed with `(n - j) ^ c`: its own inverse.
fn tail(buf: &mut [u8], c: u8) {
    let n = buf.len();
    for (j, b) in buf.iter_mut().enumerate() {
        *b ^= ((n - j) as u8) ^ c;
    }
}

/// One direction's variant.
pub struct Cipher {
    aes: Aes128,
}

impl Cipher {
    pub fn new(variant: u32, hello: &[u8; HELLO_LEN]) -> Result<Self> {
        if variant != AES128 {
            return Err(Error::Unsupported(variant));
        }
        let aes = Aes128::new_from_slice(&fold_key(hello, 16)).expect("16-byte key");
        Ok(Self { aes })
    }

    /// The client's send transform: whole blocks encrypted, then the tail.
    pub fn encrypt(&self, buf: &mut [u8]) {
        let mut blocks = buf.chunks_exact_mut(16);
        for chunk in &mut blocks {
            let mut b: [u8; 16] = (&*chunk).try_into().expect("16 bytes");
            self.aes.encrypt_block((&mut b).into());
            chunk.copy_from_slice(&b);
        }
        tail(blocks.into_remainder(), AES_TAIL);
    }

    /// The client's receive transform: whole blocks decrypted, then the tail.
    pub fn decrypt(&self, buf: &mut [u8]) {
        let mut blocks = buf.chunks_exact_mut(16);
        for chunk in &mut blocks {
            let mut b: [u8; 16] = (&*chunk).try_into().expect("16 bytes");
            self.aes.decrypt_block((&mut b).into());
            chunk.copy_from_slice(&b);
        }
        tail(blocks.into_remainder(), AES_TAIL);
    }
}
```

Run: `cargo test -p fenji-proto` → expected PASS (18 + 4 hello + 5 variant, plus any emulator tests).

- [ ] **Step 7: Gates and commit.**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo deny check
git add Cargo.toml Cargo.lock crates/fenji-proto
git commit -m "feat(proto): build and check the hello, and add the AES-128 variant" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 6: `fenji-proto`: server handshake and client simulator

**Files:**
- Create: `crates/fenji-proto/src/handshake.rs`, `crates/fenji-proto/src/client.rs`, `crates/fenji-proto/tests/handshake.rs`
- Modify: `crates/fenji-proto/src/lib.rs`

**Interfaces:**
- Consumes Tasks 3–5.
- Produces:

```rust
pub const handshake::AUTH_FIXED: usize = 22;          // 16-byte dictionary id + 6-byte machine field
pub const handshake::MAX_INBOUND: usize = 1 << 20;    // largest message body accepted from a client
pub struct handshake::AuthMessage { pub dict_id: [u8; 16], pub machine: [u8; 6], pub payload: Vec<u8> }
impl AuthMessage { pub fn to_body(&self) -> Vec<u8>; pub fn parse(body: &[u8]) -> Result<Self> }
pub struct handshake::Message { pub ty: u8, pub body: Vec<u8> }
pub enum handshake::Event { Auth(AuthMessage), TimeSync(u64), Message(Message) }
pub enum handshake::State { AwaitAuth, Authed, Queued, Closed }
pub struct handshake::Server;
impl Server {
    pub fn start(fill: [u8; 64], v: Variants) -> Result<(Self, [u8; 64])>;
    pub fn state(&self) -> State;
    pub fn receive(&mut self, bytes: &[u8]) -> Result<Vec<Event>>;
    pub fn accept(&mut self, codec: &dyn StreamCodec) -> Result<Vec<u8>>;
    pub fn reject(&mut self, reason: &[u8], codec: &dyn StreamCodec) -> Result<Vec<u8>>;
    pub fn send(&mut self, ty: u8, body: &[u8], codec: &dyn StreamCodec) -> Result<Vec<u8>>;
    pub fn time_sync(&mut self, millis: u64) -> Vec<u8>;
}
pub enum client::ClientEvent { Accepted, Rejected(Vec<u8>), TimeSync(u64), Message(Message) }
pub struct client::Client;
impl Client {
    pub fn new(auth: AuthMessage) -> Self;
    pub fn receive(&mut self, bytes: &[u8]) -> Result<(Vec<ClientEvent>, Vec<u8>)>;  // events, bytes to send
    pub fn send(&mut self, ty: u8, body: &[u8]) -> Result<Vec<u8>>;
    pub fn time_sync(&mut self, millis: u64) -> Vec<u8>;
}
```

`Client` mirrors the real client's transport. The tests use it, and later the bot client (parent spec §9). It decodes only `StoredCodec` packets.

- [ ] **Step 1: Branch.** `git switch dev && git pull --ff-only && git switch -c feat/proto-<issue>-handshake`

- [ ] **Step 2: Write the failing end-to-end tests** in `crates/fenji-proto/tests/handshake.rs`:

```rust
use fenji_proto::client::{Client, ClientEvent};
use fenji_proto::codec::StoredCodec;
use fenji_proto::handshake::{AuthMessage, Event, MAX_INBOUND, Message, Server, State};
use fenji_proto::hello::Variants;
use fenji_proto::variant::AES128;

const V: Variants = Variants { down: AES128, up: AES128 };

fn auth() -> AuthMessage {
    AuthMessage {
        dict_id: [0x11; 16],
        machine: [0x02, 0, 0, 0, 0, 0x01],
        payload: b"synthetic payload".to_vec(),
    }
}

fn fill(seed: u8) -> [u8; 64] {
    core::array::from_fn(|i| (i as u8).wrapping_mul(29).wrapping_add(seed))
}

/// Runs hello and auth; returns the pair and the server's auth event.
fn connect(seed: u8) -> (Server, Client, AuthMessage) {
    let (mut server, hello) = Server::start(fill(seed), V).unwrap();
    let mut client = Client::new(auth());
    let (events, up) = client.receive(&hello).unwrap();
    assert!(events.is_empty());
    let got = server.receive(&up).unwrap();
    let [Event::Auth(a)] = got.as_slice() else { panic!("{got:?}") };
    assert_eq!(server.state(), State::Authed);
    (server, client, a.clone())
}

#[test]
fn hello_auth_ok() {
    let (mut server, mut client, a) = connect(1);
    assert_eq!(a, auth());
    let down = server.accept(&StoredCodec).unwrap();
    assert_eq!(server.state(), State::Queued);
    let (events, _) = client.receive(&down).unwrap();
    assert_eq!(events, [ClientEvent::Accepted]);
}

#[test]
fn reject_carries_the_reason() {
    let (mut server, mut client, _) = connect(2);
    let down = server.reject(b"server full", &StoredCodec).unwrap();
    assert_eq!(server.state(), State::Closed);
    let (events, _) = client.receive(&down).unwrap();
    assert_eq!(events, [ClientEvent::Rejected(b"server full".to_vec())]);
}

#[test]
fn large_messages_cross_frames_both_ways() {
    let (mut server, mut client, _) = connect(3);
    let down = server.accept(&StoredCodec).unwrap();
    client.receive(&down).unwrap();
    let big: Vec<u8> = (0..10_000u32).map(|i| (i * 31) as u8).collect();
    let down = server.send(9, &big, &StoredCodec).unwrap();
    let (events, _) = client.receive(&down).unwrap();
    assert_eq!(events, [ClientEvent::Message(Message { ty: 9, body: big.clone() })]);
    let up = client.send(4, &big).unwrap();
    let events = server.receive(&up).unwrap();
    assert_eq!(events, [Event::Message(Message { ty: 4, body: big })]);
}

#[test]
fn time_syncs_both_ways() {
    let (mut server, mut client, _) = connect(4);
    let up = client.time_sync(123_456);
    assert_eq!(server.receive(&up).unwrap(), [Event::TimeSync(123_456)]);
    let down = server.time_sync(654_321);
    let (events, _) = client.receive(&down).unwrap();
    assert_eq!(events, [ClientEvent::TimeSync(654_321)]);
}

#[test]
fn byte_at_a_time_matches_one_read() {
    let (mut server, hello) = Server::start(fill(5), V).unwrap();
    let mut client = Client::new(auth());
    let mut up = Vec::new();
    for b in hello {
        let (events, bytes) = client.receive(&[b]).unwrap();
        assert!(events.is_empty());
        up.extend(bytes);
    }
    up.extend(client.time_sync(7));
    let mut events = Vec::new();
    for b in &up {
        events.extend(server.receive(std::slice::from_ref(b)).unwrap());
    }
    assert_eq!(events, [Event::Auth(auth()), Event::TimeSync(7)]);
    // The same bytes in one read give the same events in the same order.
    let (mut again, _) = Server::start(fill(5), V).unwrap();
    assert_eq!(again.receive(&up).unwrap(), events);
}

#[test]
fn oversized_inbound_message_is_refused() {
    let (mut server, mut client, _) = connect(6);
    let down = server.accept(&StoredCodec).unwrap();
    client.receive(&down).unwrap();
    let up = client.send(1, &vec![0; MAX_INBOUND + 1]).unwrap();
    assert!(server.receive(&up).is_err());
}

#[test]
fn junk_never_panics() {
    // A deterministic xorshift stream: 200 junk inputs of up to 4 KiB, before and after the hello.
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..200 {
        let len = (next() % 4096) as usize;
        let junk: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        let (mut server, _) = Server::start(fill(7), V).unwrap();
        let _ = server.receive(&junk);
        let mut client = Client::new(auth());
        let _ = client.receive(&junk);
    }
}

#[test]
fn send_before_accept_is_refused() {
    let (mut server, _, _) = connect(8);
    assert!(server.send(1, b"x", &StoredCodec).is_err());
}
```

Run: `cargo test -p fenji-proto --test handshake` → expected FAIL (no `client` or `handshake` modules).

- [ ] **Step 3: Implement `handshake.rs`:**

```rust
//! The server's side of the gate handshake (FUN_14029C330 and FUN_14029ADD0 on the client;
//! docs/protocol/transport.md §6). Send the hello, read the client's auth message, then accept
//! (`ok`, the client goes to `QUEUE`) or reject (the reason becomes the client's close reason).
//! Client data is transformed but never Oodle-coded; server data goes through `StreamCodec`.
use crate::codec::StreamCodec;
use crate::frame::{DownWriter, Frame, SEND_CHUNK, UpReader};
use crate::header::{HEADER_LEN, split_message, write_message};
use crate::hello::{CLIENT_MAGIC, HELLO_LEN, Variants, build};
use crate::variant::Cipher;
use crate::{Error, Result};

/// The auth body's fixed part: 16 bytes from DAT_1431FC8E0's object, then 6 machine bytes.
pub const AUTH_FIXED: usize = 22;
/// The largest message body this server takes from a client: the client's own auth payload is at
/// most 0x4000 bytes (FUN_14029A900), and nothing it sends early is known to be larger.
pub const MAX_INBOUND: usize = 1 << 20;
/// The client keeps at most this much of a close reason (spike-0 4.28).
pub const MAX_REASON: usize = 255;

/// The client's first message (type 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthMessage {
    /// From the object at DAT_1431FC8E0 (transport.md §6).
    pub dict_id: [u8; 16],
    /// The `SendARP` MAC address, or a marker when that fails (F5).
    pub machine: [u8; 6],
    /// `net_client_create`'s payload: `ks.pickle_pack` of the login fields (login.md 2.6).
    pub payload: Vec<u8>,
}

impl AuthMessage {
    pub fn to_body(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(AUTH_FIXED + self.payload.len());
        b.extend_from_slice(&self.dict_id);
        b.extend_from_slice(&self.machine);
        b.extend_from_slice(&self.payload);
        b
    }

    pub fn parse(body: &[u8]) -> Result<Self> {
        if body.len() < AUTH_FIXED {
            return Err(Error::BadAuth(format!("{} bytes, at least 22 expected", body.len())));
        }
        Ok(Self {
            dict_id: body[..16].try_into().expect("16 bytes"),
            machine: body[16..22].try_into().expect("6 bytes"),
            payload: body[22..].to_vec(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub ty: u8,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Auth(AuthMessage),
    TimeSync(u64),
    Message(Message),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    AwaitAuth,
    Authed,
    Queued,
    Closed,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            Self::AwaitAuth => "AwaitAuth",
            Self::Authed => "Authed",
            Self::Queued => "Queued",
            Self::Closed => "Closed",
        }
    }
}

/// Splits whole messages off the front of `stream`. Refuses a header that announces more than
/// `MAX_INBOUND` bytes before waiting for them.
pub(crate) fn drain_messages(stream: &mut Vec<u8>) -> Result<Vec<Message>> {
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        if let Some(h) = stream.get(at..at + HEADER_LEN) {
            let len = crate::header::Header::from_bytes(h.try_into().expect("4 bytes")).len;
            if len > MAX_INBOUND {
                return Err(Error::BodyTooLong(len));
            }
        }
        let Some((h, body, used)) = split_message(&stream[at..]) else { break };
        out.push(Message { ty: h.ty, body: body.to_vec() });
        at += used;
    }
    stream.drain(..at);
    Ok(out)
}

/// Encodes `msg` into frames: chunks of at most `SEND_CHUNK` raw bytes, each through `codec`, then
/// the variant transform.
pub(crate) fn frame_stream(
    msg: &[u8],
    codec: &dyn StreamCodec,
    transform: impl Fn(&mut [u8]),
    writer: &mut DownWriter,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for chunk in msg.chunks(SEND_CHUNK) {
        let mut payload = Vec::with_capacity(chunk.len() + 5);
        codec.encode(chunk, &mut payload)?;
        transform(&mut payload);
        writer.data(&payload, &mut out)?;
    }
    Ok(out)
}

pub struct Server {
    state: State,
    down_cipher: Cipher,
    up_cipher: Cipher,
    down: DownWriter,
    up: UpReader,
    raw_in: Vec<u8>,
    stream_in: Vec<u8>,
}

impl Server {
    /// A session, and the hello to send first. `fill` should be fresh random bytes.
    pub fn start(fill: [u8; HELLO_LEN], v: Variants) -> Result<(Self, [u8; HELLO_LEN])> {
        let hello = build(fill, v, CLIENT_MAGIC)?;
        let server = Self {
            state: State::AwaitAuth,
            down_cipher: Cipher::new(v.down, &hello)?,
            up_cipher: Cipher::new(v.up, &hello)?,
            down: DownWriter::new(),
            up: UpReader::new(),
            raw_in: Vec::new(),
            stream_in: Vec::new(),
        };
        Ok((server, hello))
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Feeds bytes read from the client; returns the events they complete. After an error the
    /// session is `Closed`.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<Vec<Event>> {
        let got = self.receive_inner(bytes);
        if got.is_err() {
            self.state = State::Closed;
        }
        got
    }

    fn receive_inner(&mut self, bytes: &[u8]) -> Result<Vec<Event>> {
        if self.state == State::Closed {
            return Err(self.wrong("receive"));
        }
        self.raw_in.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut at = 0;
        // Messages are taken frame by frame, so events keep the client's order however the bytes
        // were split into reads.
        while let Some((frame, used)) = self.up.read(&self.raw_in[at..]) {
            at += used;
            match frame {
                Frame::TimeSync(ms) => events.push(Event::TimeSync(ms)),
                Frame::Data(mut body) => {
                    self.up_cipher.decrypt(&mut body);
                    self.stream_in.extend_from_slice(&body);
                    for m in drain_messages(&mut self.stream_in)? {
                        events.push(self.take(m)?);
                    }
                }
            }
        }
        self.raw_in.drain(..at);
        Ok(events)
    }

    fn take(&mut self, m: Message) -> Result<Event> {
        match self.state {
            State::AwaitAuth if m.ty == 0 => {
                self.state = State::Authed;
                Ok(Event::Auth(AuthMessage::parse(&m.body)?))
            }
            State::AwaitAuth => Err(Error::BadAuth(format!("first message has type {}", m.ty))),
            State::Queued => Ok(Event::Message(m)),
            _ => Err(self.wrong("take a message")),
        }
    }

    /// Accepts the auth: the 2-byte body `ok`, which moves the client to `QUEUE`.
    pub fn accept(&mut self, codec: &dyn StreamCodec) -> Result<Vec<u8>> {
        if self.state != State::Authed {
            return Err(self.wrong("accept"));
        }
        let out = self.write(0, b"ok", codec)?;
        self.state = State::Queued;
        Ok(out)
    }

    /// Refuses the auth. The client shows `reason` (at most 255 bytes kept) and closes.
    pub fn reject(&mut self, reason: &[u8], codec: &dyn StreamCodec) -> Result<Vec<u8>> {
        if self.state != State::Authed {
            return Err(self.wrong("reject"));
        }
        let reason = &reason[..reason.len().min(MAX_REASON)];
        // A 2-byte reason "ok" would be read as acceptance.
        if reason == b"ok" {
            return Err(Error::BadAuth("a reason of \"ok\" would accept".into()));
        }
        let out = self.write(0, reason, codec)?;
        self.state = State::Closed;
        Ok(out)
    }

    /// One message to a client in `Queued`.
    pub fn send(&mut self, ty: u8, body: &[u8], codec: &dyn StreamCodec) -> Result<Vec<u8>> {
        if self.state != State::Queued {
            return Err(self.wrong("send"));
        }
        self.write(ty, body, codec)
    }

    pub fn time_sync(&mut self, millis: u64) -> Vec<u8> {
        let mut out = Vec::new();
        self.down.time_sync(millis, &mut out);
        out
    }

    fn write(&mut self, ty: u8, body: &[u8], codec: &dyn StreamCodec) -> Result<Vec<u8>> {
        let mut msg = Vec::with_capacity(HEADER_LEN + body.len());
        write_message(ty, body, &mut msg)?;
        let cipher = &self.down_cipher;
        frame_stream(&msg, codec, |b| cipher.encrypt(b), &mut self.down)
    }

    fn wrong(&self, action: &'static str) -> Error {
        Error::WrongState {
            action,
            state: self.state.name(),
        }
    }
}
```

Note: `frame_stream` takes a `DownWriter`, which the client simulator can't use for its upstream frames, so the client writes its own loop (Step 4). Keep `frame_stream` `pub(crate)`.

- [ ] **Step 4: Implement `client.rs`:**

```rust
//! A stand-in for the real client's transport (FUN_14029ADD0 and its callees), for tests and the
//! bot client (parent spec §9). It reads only `StoredCodec` packets: it has no Oodle decoder.
use crate::codec::{StoredCodec, StreamCodec};
use crate::frame::{DownReader, Frame, SEND_CHUNK, UpWriter};
use crate::handshake::{AuthMessage, MAX_REASON, Message, drain_messages};
use crate::header::write_message;
use crate::hello::{CLIENT_MAGIC, HELLO_LEN, check};
use crate::variant::Cipher;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEvent {
    Accepted,
    Rejected(Vec<u8>),
    TimeSync(u64),
    Message(Message),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CState {
    AwaitHello,
    InAuth,
    Queued,
    Closed,
}

pub struct Client {
    auth: AuthMessage,
    state: CState,
    down_cipher: Option<Cipher>,
    up_cipher: Option<Cipher>,
    down: DownReader,
    up: UpWriter,
    raw_in: Vec<u8>,
    stream_in: Vec<u8>,
}

impl Client {
    pub fn new(auth: AuthMessage) -> Self {
        Self {
            auth,
            state: CState::AwaitHello,
            down_cipher: None,
            up_cipher: None,
            down: DownReader::new(),
            up: UpWriter::new(),
            raw_in: Vec::new(),
            stream_in: Vec::new(),
        }
    }

    /// Feeds bytes from the server: the events, and the bytes the client sends in reply.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<(Vec<ClientEvent>, Vec<u8>)> {
        self.raw_in.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut out = Vec::new();
        if self.state == CState::AwaitHello {
            let Some(h) = self.raw_in.get(..HELLO_LEN) else {
                return Ok((events, out));
            };
            let hello: [u8; HELLO_LEN] = h.try_into().expect("64 bytes");
            let v = check(&hello, CLIENT_MAGIC)?;
            self.down_cipher = Some(Cipher::new(v.down, &hello)?);
            self.up_cipher = Some(Cipher::new(v.up, &hello)?);
            self.raw_in.drain(..HELLO_LEN);
            self.state = CState::InAuth;
            let body = self.auth.to_body();
            out = self.frames(0, &body)?;
        }
        let mut at = 0;
        while let Some((frame, used)) = self.down.read(&self.raw_in[at..]) {
            at += used;
            match frame {
                Frame::TimeSync(ms) => events.push(ClientEvent::TimeSync(ms)),
                Frame::Data(mut body) => {
                    self.down_cipher.as_ref().expect("after hello").decrypt(&mut body);
                    let raw = StoredCodec.decode(&body)?;
                    self.stream_in.extend_from_slice(&raw);
                    for m in drain_messages(&mut self.stream_in)? {
                        events.push(self.take(m)?);
                    }
                }
            }
        }
        self.raw_in.drain(..at);
        Ok((events, out))
    }

    fn take(&mut self, m: Message) -> Result<ClientEvent> {
        match self.state {
            CState::InAuth if m.body == b"ok" => {
                self.state = CState::Queued;
                Ok(ClientEvent::Accepted)
            }
            CState::InAuth => {
                self.state = CState::Closed;
                let n = m.body.len().min(MAX_REASON);
                Ok(ClientEvent::Rejected(m.body[..n].to_vec()))
            }
            CState::Queued => Ok(ClientEvent::Message(m)),
            _ => Err(Error::WrongState {
                action: "take a message",
                state: "Closed",
            }),
        }
    }

    /// One message to the server (the real client sends only in `WORK`; this one in `Queued`).
    pub fn send(&mut self, ty: u8, body: &[u8]) -> Result<Vec<u8>> {
        if self.state != CState::Queued {
            return Err(Error::WrongState {
                action: "send",
                state: "not Queued",
            });
        }
        self.frames(ty, body)
    }

    pub fn time_sync(&mut self, millis: u64) -> Vec<u8> {
        let mut out = Vec::new();
        self.up.time_sync(millis, &mut out);
        out
    }

    fn frames(&mut self, ty: u8, body: &[u8]) -> Result<Vec<u8>> {
        let mut msg = Vec::new();
        write_message(ty, body, &mut msg)?;
        let cipher = self.up_cipher.as_ref().expect("after hello");
        let mut out = Vec::new();
        for chunk in msg.chunks(SEND_CHUNK) {
            let mut c = chunk.to_vec();
            cipher.encrypt(&mut c);
            self.up.data(&c, &mut out)?;
        }
        Ok(out)
    }
}
```

The real client checks the `ok` length (2) and body. `m.body == b"ok"` is the same rule. Add to `lib.rs`: `pub mod client;` and `pub mod handshake;`.

Run: `cargo test -p fenji-proto` → expected PASS (all unit tests plus 8 in `tests/handshake.rs`).

- [ ] **Step 5: Gates and commit.**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked
git add crates/fenji-proto
git commit -m "feat(proto): add the server handshake and a client simulator" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 7: `fenji-stub --speak`, the events log, and `run-client.sh --stub-arg`

**Files:**
- Create: `tools/stub/src/speak.rs`, `tools/stub/src/events.rs`, `tools/stub/tests/speak.rs`
- Modify: `tools/stub/src/{lib,main,server,session}.rs`, `tools/stub/tests/serve.rs`, `tools/stub/Cargo.toml`, `scripts/spike/run-client.sh`, `scripts/spike/test-run-client.sh`

**Interfaces:**
- Consumes `fenji_proto::{handshake::{Server, Event}, codec::StoredCodec, hello::Variants, variant::AES128}`.
- Produces:
  - `fenji-stub --listen ADDR --speak [--down-variant N] [--up-variant N] [--auth-reply ok|none]`;
  - next to each `<stem>.log`, a `<stem>.events` (tab-separated `millis kind detail`) and a `<stem>.auth.bin` (the auth body, local only);
  - `run-client.sh run … --stub-arg ARG`, repeatable.

- [ ] **Step 1: Branch, add the dependency, and change `serve`'s signature.** `git switch dev && git pull --ff-only && git switch -c feat/tools-<issue>-stub-speak`. In `tools/stub/Cargo.toml` `[dependencies]`: `fenji-proto.workspace = true`.

In `server.rs`, `serve` takes a mode:

```rust
/// What the stub does with a connection.
#[derive(Debug, Clone)]
pub enum Mode {
    /// Log what the client sends and never write (M0–M1 behaviour).
    Listen,
    /// Speak the transport: hello, auth, reply (M2 spec §5).
    Speak(crate::speak::SpeakOpts),
}

pub async fn serve(listener: TcpListener, out_dir: PathBuf, mode: Mode) -> io::Result<()> {
```

Inside the accept loop, clone `mode` into the task and dispatch:

```rust
        let mode = mode.clone();
        tokio::spawn(async move {
            let result = match mode {
                Mode::Listen => handle(stream, local, peer, &out_dir).await,
                Mode::Speak(opts) => crate::speak::handle(stream, local, peer, &out_dir, opts).await,
            };
            if let Err(error) = result {
                tracing::warn!(%peer, %error, "session ended with error");
            }
        });
```

Update `tests/serve.rs`'s `start` to call `serve(listener, dir.to_path_buf(), fenji_stub::server::Mode::Listen)`. Run `cargo test -p fenji-stub`: it fails to compile until Steps 2–4 add `speak`.

- [ ] **Step 2: `SessionLog::record_sent`, test first.** Add to `session.rs`'s tests:

```rust
    #[test]
    fn sent_bytes_have_their_own_offsets() {
        let tmp = tempfile::tempdir().unwrap();
        let (local, peer) = addrs();
        let mut log = SessionLog::create(tmp.path(), local, peer).unwrap();
        log.record_sent(b"hi").unwrap();
        log.record(b"abc").unwrap();
        log.record_sent(b"!").unwrap();
        let path = log.path().to_path_buf();
        log.close("peer closed").unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("# sent 2 bytes at offset 0\n00000000  68 69"));
        assert!(text.contains("# recv 3 bytes at offset 0\n00000000  61 62 63"));
        assert!(text.contains("# sent 1 bytes at offset 2\n00000002  21"));
    }
```

Then add the field `sent: usize` (initialised to 0 in `create`) and:

```rust
    /// Appends bytes the stub sent, offset-continuous with earlier sends.
    pub fn record_sent(&mut self, bytes: &[u8]) -> io::Result<()> {
        writeln!(self.file, "# sent {} bytes at offset {}", bytes.len(), self.sent)?;
        self.file.write_all(hexdump(self.sent, bytes).as_bytes())?;
        self.sent += bytes.len();
        self.file.flush()
    }
```

- [ ] **Step 3: `events.rs`:**

```rust
//! The decoded side of a `--speak` session: one tab-separated line per event, `millis kind detail`,
//! with the time since the connection opened. The raw hexdump log stays the ground truth.
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::time::Instant;

pub struct EventLog {
    file: File,
    start: Instant,
}

impl EventLog {
    /// Creates `path`; never overwrites.
    pub fn create(path: &Path) -> io::Result<Self> {
        let file = File::options().write(true).create_new(true).open(path)?;
        Ok(Self {
            file,
            start: Instant::now(),
        })
    }

    pub fn line(&mut self, kind: &str, detail: &str) -> io::Result<()> {
        let ms = self.start.elapsed().as_millis();
        writeln!(self.file, "{ms}\t{kind}\t{detail}")?;
        self.file.flush()
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
```

- [ ] **Step 4: `speak.rs`:**

```rust
//! `--speak`: the stub's side of the gate handshake, driven by `fenji_proto::handshake::Server`
//! (M2 spec §5). Raw bytes in both directions go to the session log; decoded events go to
//! `<stem>.events`; the auth body goes to `<stem>.auth.bin` for analysis (all under re/).
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use fenji_proto::codec::StoredCodec;
use fenji_proto::handshake::{Event, Server};
use fenji_proto::hello::{HELLO_LEN, Variants};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::events::{EventLog, hex};
use crate::session::SessionLog;

/// What to answer the client's auth message with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthReply {
    /// `ok` in a stored frame (rung R3).
    Ok,
    /// Nothing: stop after reading the auth (rungs R1–R2).
    None,
}

#[derive(Debug, Clone, Copy)]
pub struct SpeakOpts {
    pub variants: Variants,
    pub auth_reply: AuthReply,
}

/// Fresh bytes for the hello. The hello goes out in clear and the keys are folded from it, so it
/// needs to differ per connection, not to be secret: std's per-process random hasher keys plus the
/// clock are enough.
fn fill() -> [u8; HELLO_LEN] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut out = [0u8; HELLO_LEN];
    for (i, chunk) in out.chunks_mut(8).enumerate() {
        let mut h = RandomState::new().build_hasher();
        h.write_usize(i);
        h.write_u128(nanos);
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    out
}

fn other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

pub async fn handle(
    mut stream: TcpStream,
    local: SocketAddr,
    peer: SocketAddr,
    out_dir: &Path,
    opts: SpeakOpts,
) -> io::Result<()> {
    let mut log = SessionLog::create(out_dir, local, peer)?;
    let mut events = EventLog::create(&log.path().with_extension("events"))?;
    tracing::info!(%peer, %local, log = %log.path().display(), "connection (speak)");
    let (mut server, hello) = Server::start(fill(), opts.variants).map_err(other)?;
    stream.write_all(&hello).await?;
    log.record_sent(&hello)?;
    events.line(
        "hello",
        &format!("down={} up={}", opts.variants.down, opts.variants.up),
    )?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut decoding = true;
    loop {
        let n = match stream.read(&mut buf).await {
            Ok(0) => {
                events.line("closed", "peer closed")?;
                return log.close("peer closed");
            }
            Ok(n) => n,
            Err(error) => {
                events.line("closed", &format!("read error: {error}"))?;
                log.close(&format!("read error: {error}"))?;
                return Err(error);
            }
        };
        log.record(&buf[..n])?;
        if !decoding {
            continue;
        }
        let got = match server.receive(&buf[..n]) {
            Ok(got) => got,
            Err(error) => {
                // Keep logging raw bytes; stop decoding a stream we no longer follow.
                events.line("error", &error.to_string())?;
                decoding = false;
                continue;
            }
        };
        for event in got {
            match event {
                Event::Auth(a) => {
                    events.line(
                        "auth",
                        &format!(
                            "dict_id={} machine={} payload_len={}",
                            hex(&a.dict_id),
                            hex(&a.machine),
                            a.payload.len()
                        ),
                    )?;
                    std::fs::write(log.path().with_extension("auth.bin"), a.to_body())?;
                    if opts.auth_reply == AuthReply::Ok {
                        let out = server.accept(&StoredCodec).map_err(other)?;
                        stream.write_all(&out).await?;
                        log.record_sent(&out)?;
                        events.line("sent", "ok (stored frame)")?;
                    }
                }
                Event::TimeSync(ms) => events.line("time_sync", &format!("client_ms={ms}"))?,
                Event::Message(m) => {
                    let head = &m.body[..m.body.len().min(32)];
                    events.line(
                        "message",
                        &format!("ty={} len={} head={}", m.ty, m.body.len(), hex(head)),
                    )?;
                }
            }
        }
    }
}
```

In `lib.rs`, add `pub mod events;` and `pub mod speak;`.

- [ ] **Step 5: CLI flags** in `main.rs`, added to `Args`:

```rust
    /// Speak the client's transport: send the hello, read the auth, reply (M2 spec §5)
    #[arg(long)]
    speak: bool,
    /// Variant for server-to-client frames (--speak); only 3, AES-128, is implemented
    #[arg(long, default_value_t = fenji_proto::variant::AES128)]
    down_variant: u32,
    /// Variant for client-to-server frames (--speak)
    #[arg(long, default_value_t = fenji_proto::variant::AES128)]
    up_variant: u32,
    /// Reply to the auth message: ok (a stored frame) or none
    #[arg(long, default_value = "ok", value_parser = ["ok", "none"])]
    auth_reply: String,
```

and build the mode before the listeners:

```rust
    let mode = if args.speak {
        fenji_stub::server::Mode::Speak(fenji_stub::speak::SpeakOpts {
            variants: fenji_proto::hello::Variants {
                down: args.down_variant,
                up: args.up_variant,
            },
            auth_reply: if args.auth_reply == "ok" {
                fenji_stub::speak::AuthReply::Ok
            } else {
                fenji_stub::speak::AuthReply::None
            },
        })
    } else {
        fenji_stub::server::Mode::Listen
    };
```

Pass `mode.clone()` to each `serve`. Update the `Args` doc comment: "Accepts game-client connections and logs everything received; with --speak, also speaks the transport."

- [ ] **Step 6: The integration test,** `tools/stub/tests/speak.rs`:

```rust
use std::path::Path;
use std::time::Duration;

use fenji_proto::client::{Client, ClientEvent};
use fenji_proto::handshake::AuthMessage;
use fenji_proto::hello::Variants;
use fenji_proto::variant::AES128;
use fenji_stub::server::{Mode, serve};
use fenji_stub::speak::{AuthReply, SpeakOpts};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn start(dir: &Path, auth_reply: AuthReply) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let opts = SpeakOpts {
        variants: Variants { down: AES128, up: AES128 },
        auth_reply,
    };
    tokio::spawn(serve(listener, dir.to_path_buf(), Mode::Speak(opts)));
    addr
}

fn auth() -> AuthMessage {
    AuthMessage {
        dict_id: [0x22; 16],
        machine: [0x02, 0, 0, 0, 0, 0x01],
        payload: b"test".to_vec(),
    }
}

/// Reads until `client` reports events or `ms` pass.
async fn pump(s: &mut TcpStream, client: &mut Client, ms: u64) -> Vec<ClientEvent> {
    let mut buf = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    loop {
        let read = tokio::time::timeout_at(deadline, s.read(&mut buf)).await;
        let Ok(Ok(n)) = read else { return Vec::new() };
        if n == 0 {
            return Vec::new();
        }
        let (events, out) = client.receive(&buf[..n]).unwrap();
        if !out.is_empty() {
            s.write_all(&out).await.unwrap();
        }
        if !events.is_empty() {
            return events;
        }
    }
}

fn read_events(dir: &Path) -> String {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "events"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect()
}

#[tokio::test]
async fn speaks_hello_auth_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path(), AuthReply::Ok).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut client = Client::new(auth());
    assert_eq!(pump(&mut s, &mut client, 5000).await, [ClientEvent::Accepted]);
    s.write_all(&client.time_sync(99)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let ev = read_events(tmp.path());
    assert!(ev.contains("\thello\tdown=3 up=3"), "{ev}");
    assert!(ev.contains("\tauth\tdict_id=22222222222222222222222222222222 machine=020000000001 payload_len=4"), "{ev}");
    assert!(ev.contains("\tsent\tok (stored frame)"), "{ev}");
    assert!(ev.contains("\ttime_sync\tclient_ms=99"), "{ev}");
}

#[tokio::test]
async fn auth_reply_none_stays_silent() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path(), AuthReply::None).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut client = Client::new(auth());
    assert!(pump(&mut s, &mut client, 500).await.is_empty());
    let ev = read_events(tmp.path());
    assert!(ev.contains("\tauth\t"), "{ev}");
    assert!(!ev.contains("\tsent\t"), "{ev}");
}
```

Run: `cargo test -p fenji-stub` → expected PASS (the old tests, the new session test and these two).

- [ ] **Step 7: `run-client.sh --stub-arg`.**
  - Header comment: add `[--stub-arg ARG]...` to the `run` synopsis, with the line "`--stub-arg` passes one word to fenji-stub, e.g. `--stub-arg --speak` (M2)."
  - In `cmd_run`: `local envs=() client_files=() clicks=() stub_args=()`, and the option `--stub-arg) [ $# -ge 2 ] || usage; stub_args+=("$2"); shift 2 ;;`.
  - Validate each one next to the `--env` checks:

```bash
  for sa in "${stub_args[@]}"; do
    case "$sa" in ''|*[!A-Za-z0-9=_.:,-]*) die "--stub-arg takes one word of [A-Za-z0-9=_.:,-], not '$sa'" ;; esac
  done
```

  - In the sandbox's inner script, take an eighth fixed argument and pass it, word-split on purpose:

```bash
      W=$1 IO=$2 STUB=$3 STUBBIN=$4 PORTS=$5 TMO=$6 PROTON=$7 SARGS=$8
      shift 8
      …
      # shellcheck disable=SC2086 # SARGS holds validated words, split on purpose
      "$STUBBIN/fenji-stub" "${la[@]}" $SARGS --out "$STUB" > "$IO/stub.out" 2>&1 &
```

  - At the call site, add `"${stub_args[*]}"` after `"$PROTON"`: `… "$tmo" "$PROTON" "${stub_args[*]}" "$@"`.
  - In `test-run-client.sh`, next to the `--ports` case:

```bash
refuses "--stub-arg takes one word" "--stub-arg" -- --stub-arg 'a b'
refuses "--stub-arg refuses shell characters" "--stub-arg" -- --stub-arg '--x;rm'
```

Run: `shellcheck scripts/spike/run-client.sh scripts/spike/test-run-client.sh && scripts/spike/test-run-client.sh` → expected: all `ok`, no `FAIL`.

- [ ] **Step 8: Gates and commit.**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked
git add tools/stub scripts/spike/run-client.sh scripts/spike/test-run-client.sh Cargo.lock
git commit -m "feat(tools): let fenji-stub speak the client's handshake" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 8: Rungs R1–R3: the hello, the auth fixture, and the stored-frame Oodle test

**Files:**
- Modify: `docs/protocol/transport.md` (a section "§9. Runs")
- Create: `crates/fenji-proto/tests/fixtures/r2-up.hex`, `crates/fenji-proto/tests/fixtures/r2-hello.hex`, `crates/fenji-proto/tests/fixtures.rs`
- Local only: `re/runs/out/m2-r*/`, `re/stub-logs/m2-r*/`, `re/m2/runs.md`

**Interfaces:**
- Consumes Task 7's `--speak` and `--stub-arg`.
- Produces:
  - R1's verdict (the hello is accepted);
  - R2's scrubbed fixture of the client's real hello reply;
  - R3's verdict on P1: does the client accept a stored frame? A `yes` unblocks Task 10. A `no` opens gate G2.

This is RE work: every run is a client run. Clean session only, inside `run-client.sh`. Before each run, write its prediction in `re/m2/runs.md`.

The client arguments, the launcher path as in run en-1 (P4; `re/m1/connect/runs.md`), used by every rung here:

```bash
CLIENT_ARGS=(--gf-sdk=mocked --fs=data:_index:_index gj_gates=127.0.0.1:7800 gj_ticket=fenji-dev)
```

If `re/m1/connect/en-1.console` or `runs.md` shows en-1 ran without `--gf-sdk=mocked`, drop that flag here so the runs match.

- [ ] **Step 1: Preconditions.**

```bash
git switch dev && git pull --ff-only && git switch -c docs/re-<issue>-rungs-r1-r3
cargo build --release -p fenji-stub
pgrep -x steam && echo "quit Steam first"; test -S /tmp/.X11-unix/X99 || echo "start the nested display (docs/re/README.md)"
mkdir -p re/m2
```

- [ ] **Step 2: R1 and R2 in one run: hello, then read the auth, no reply.** Prediction: the client doesn't close with 0x5F5E104; within a second of the hello it sends one data frame (the auth) and then time syncs about every 2 s; the stub's `.events` shows `auth`.

```bash
scripts/spike/run-client.sh run m2-r1 --timeout 180 --ports 7800 \
  --env gj_uinfo=1:90:Fenji:0:tester:1 \
  --stub-arg --speak --stub-arg --auth-reply=none \
  -- "${CLIENT_ARGS[@]}"; echo "exit $?"
cat re/stub-logs/m2-r1/*.events
```

Expected: exit 0, and `.events` lines `hello`, `auth … payload_len=N` and repeated `time_sync`. If the events show `error` or no `auth`:
1. Read the raw log (`re/stub-logs/m2-r1/*.log`): did the client send anything after the hello?
2. Read `re/runs/out/m2-r1/gujianol.log` for a close reason.
3. Go back to Task 1's reading of the stage that failed.

Change one thing per run (`m2-r1b`, …), each recorded in `runs.md`.

- [ ] **Step 3: Make the scrubbed fixture.** From R1's raw log, take the 64 sent bytes (the hello) and the client's received bytes up to the end of the auth message's frames. Write a throwaway script in `re/m2/` (never committed) that:
  1. decrypts the auth frames with the hello (using `fenji_proto` from a scratch test, or Python mirroring Task 5);
  2. replaces the 6 machine bytes with `02 00 00 00 00 01` and the payload with zeros of the same length;
  3. re-encrypts the result with the same hello, keeping the original frame boundaries.

  Save the hello as `crates/fenji-proto/tests/fixtures/r2-hello.hex` and the scrubbed client bytes as `r2-up.hex`, both as plain hex, 32 bytes per line. Record the original payload length and the 16-byte `dict_id` in `runs.md`. The `dict_id` comes from the client's own data, not the machine, so the fixture keeps it.

- [ ] **Step 4: The fixture test,** `crates/fenji-proto/tests/fixtures.rs`:

```rust
//! Real-client fixtures from sandboxed runs (docs/protocol/transport.md §9), scrubbed: the machine
//! bytes and the auth payload are synthetic.
use fenji_proto::frame::{Frame, UpReader};
use fenji_proto::handshake::AuthMessage;
use fenji_proto::header::split_message;
use fenji_proto::hello::{CLIENT_MAGIC, check};
use fenji_proto::variant::Cipher;

fn unhex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn r2_client_auth_decodes() {
    let hello: [u8; 64] = unhex(include_str!("fixtures/r2-hello.hex")).try_into().unwrap();
    let up = unhex(include_str!("fixtures/r2-up.hex"));
    let v = check(&hello, CLIENT_MAGIC).unwrap();
    let cipher = Cipher::new(v.up, &hello).unwrap();
    let mut reader = UpReader::new();
    let mut stream = Vec::new();
    let mut at = 0;
    while let Some((frame, used)) = reader.read(&up[at..]) {
        at += used;
        if let Frame::Data(mut body) = frame {
            cipher.decrypt(&mut body);
            stream.extend(body);
        }
    }
    let (h, body, _) = split_message(&stream).expect("one whole message");
    assert_eq!(h.ty, 0);
    let auth = AuthMessage::parse(body).unwrap();
    assert!(!auth.payload.is_empty());
}

#[test]
fn fixture_is_scrubbed() {
    let hello: [u8; 64] = unhex(include_str!("fixtures/r2-hello.hex")).try_into().unwrap();
    let up = unhex(include_str!("fixtures/r2-up.hex"));
    let v = check(&hello, CLIENT_MAGIC).unwrap();
    let cipher = Cipher::new(v.up, &hello).unwrap();
    let mut reader = UpReader::new();
    let mut stream = Vec::new();
    let mut at = 0;
    while let Some((frame, used)) = reader.read(&up[at..]) {
        at += used;
        if let Frame::Data(mut body) = frame {
            cipher.decrypt(&mut body);
            stream.extend(body);
        }
    }
    let (_, body, _) = split_message(&stream).unwrap();
    let auth = AuthMessage::parse(body).unwrap();
    assert_eq!(auth.machine, [0x02, 0, 0, 0, 0, 0x01]);
    assert!(auth.payload.iter().all(|&b| b == 0), "payload must be zeroed until Task 10");
}
```

Run: `cargo test -p fenji-proto --test fixtures` → expected PASS. This test is the golden check of Tasks 4–6 against the real client: frame keys, AES variant, fold, header and auth layout. If it fails, find the stage that differs (frame lengths, decryption, header), correct the `fenji-proto` code with a test, and record the correction in `transport.md`.

- [ ] **Step 5: R3: `ok` in a stored frame.** Prediction, written first: either (a) the client accepts it: no close with 0x5F5E101 or 0x5F5E106, it stays connected and keeps sending time syncs, and `gujianol.log` shows no network error; or (b) it closes, with the stub logging `closed peer closed` within a second of `sent ok` and `gujianol.log` naming the error.

```bash
scripts/spike/run-client.sh run m2-r3 --timeout 180 --ports 7800 \
  --env gj_uinfo=1:90:Fenji:0:tester:1 \
  --stub-arg --speak --stub-arg --auth-reply=ok \
  -- "${CLIENT_ARGS[@]}"; echo "exit $?"
cat re/stub-logs/m2-r3/*.events; grep -iE 'net|close|error|5f5e|queue' re/runs/out/m2-r3/gujianol.log | head -40
```

Judge (a) or (b) from the evidence, and run it a second time (`m2-r3b`) to confirm whichever it is.

- [ ] **Step 6: Act on R3.**
  - **(a) accepted:** P1 is settled with no encoder and no gate. Write it in `transport.md` §5 and §9 (Observed), and note it in `re/m2/progress.md`. Task 10 can proceed.
  - **(b) rejected:** open gate G2:

```bash
gh issue create --title "Decision gate: the Oodle Network wire path" \
  --label decision-gate --label status:blocked --label type:re --milestone "M2 Transport + handshake" \
  --body-file re/m2/oodle-wire-gate.md
```

    The body follows the decision-gate template:
    - **What we found:** R3's evidence; F4.
    - **Options**, each with its legal and provenance risk and effort:
      - (a) a clean-room OodleNetwork1 TCP encoder built by analysing the client's decoder (rule 6: analysing middleware);
      - (b) the DLL's own `OodleNetwork1TCP_Encode` in a sandboxed helper for dev, with server use as a separate decision;
      - any route Task 9 finds.
    - **Recommendation**, then **Decision**, left for the owner.

    Tell the owner with the link. Task 10 waits; Tasks 9 and 11–13 go on.

- [ ] **Step 7: Write `transport.md` §9 "Runs":** a table of runs (name, change, events, verdict, watchdog, exit), then Observed / Inferred / Unknown bullets and the evidence keys. Commit and merge:

```bash
scripts/check-forbidden-files.sh
git add docs/protocol/transport.md crates/fenji-proto/tests
git commit -m "docs(re): take the real client through the hello and auth" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 9: Post-auth reading: `QUEUE` → `WORK`, opcodes, the payload encoding, time sync

**Files:**
- Modify: `docs/protocol/transport.md` (§7, §8, and a new §10 "After auth")
- Create: `docs/protocol/opcodes.md`
- Modify: this plan (append "Amendment A: Task 10's code")
- Local only: `re/m2/post-auth/`

**Interfaces:**
- Consumes F7, Task 1's `pickle_pack` address, R1's `.auth.bin` (`re/stub-logs/m2-r1/`), and the M1 script listings (`re/m1/disasm/data/core/client/net_manager.luac.dis` and the decompiled `net_manager.lua`).
- Produces:
  - what the server must send to move the client from `QUEUE` to `WORK`;
  - the wire values of the `STC_PROTOCOL_*` and `CTS_PROTOCOL_*` opcodes;
  - the payload encoding (`ks.pickle_pack` or other);
  - whether the server must answer time syncs;
  - **Amendment A**, which gives Task 10 exact files, types, tests and code.

This is RE work: a clean session only. Read-only Ghidra; no client runs in this task.

- [ ] **Step 1: Branch.** `git switch dev && git pull --ff-only && git switch -c docs/re-<issue>-post-auth && mkdir -p re/m2/post-auth`

- [ ] **Step 2: `QUEUE` → `WORK`.**
  - Read FUN_140222AD0 (`net_client_dispatch`) whole, with its callees FUN_14029A8E0, FUN_14029A8D0 and FUN_14029AAB0. Find what posts the events of kinds 2, 5 and 6 that it handles: `get_xrefs_to` on the queue DAT_1431F3EC0 and on the poster FUN_1400BD1B0.
  - Find every write of 3 to the client state (`*ctx = 3`). Use `search_instructions` for `MOV dword ptr [R*], 0x3` in the 0x140299000–0x14029D000 range, or the state's readers.
  - Then read the Lua side in `net_manager.luac`: what it does with `net_client_dispatch`'s return values (2, 3) and with each message.
  - Write down exactly what the server must send, or do, for the client to reach `WORK`.

- [ ] **Step 3: Opcode wire values.**
  - Find where an inbound message's type is dispatched: the reader of the 0x390 queue in state 2/3, through `net_client_dispatch` into Lua or a native table.
  - Map each `STC_PROTOCOL_*` name (spike-0 4.12) to the number on the wire, and each `CTS_PROTOCOL_*` name to the number the client sends: its callers of the send function FUN_14029AC60 (spike-0 4.29) and the Lua constants passed to the sending binding.
  - Record each value with its evidence. The type in the inner header (F6) is the opcode, unless the reading shows a second-level type inside the body.

- [ ] **Step 4: The payload encoding.**
  - Read `ks.pickle_pack` (Task 1, Step 3) and its unpacker: the type tags, the integer and string encodings, tables or maps, and the byte order.
  - Decode R1's `re/stub-logs/m2-r1/*.auth.bin` payload (bytes 22 onwards) by hand, or with a throwaway script in `re/m2/post-auth/`, against that reading, and list its fields: name, type, and which ones identify the machine. Nothing identifying goes into `docs/`.
  - Check whether `SCRIPT` and `GATE_SCRIPT` bodies use the same encoding (their senders in `net_manager.luac`).

- [ ] **Step 5: Time sync.** Read whether the client closes or misbehaves without server time syncs (a timeout on ctx+0x3B8 or ctx+0x3B0), and what it does with them (`net_client_get_server_time`). Decide whether `--speak` must send them, and how often.

- [ ] **Step 6: Write it up.**
  - `transport.md` §10 "After auth": `QUEUE` → `WORK`, dispatch, time sync.
  - `docs/protocol/opcodes.md`, in the ledger format of `docs/protocol/README.md`: every opcode with its value, direction, client name, handler, status (`named` or `schema`; `verified` is Task 10's) and evidence.
  - `docs/protocol/README.md`: link `opcodes.md`.

- [ ] **Step 7: Write Amendment A** at the end of this plan, under the heading `## Amendment A: Task 10's code (from Task 9)`, in this plan's task format: files, interfaces, failing tests with code, implementation code, commands. It must cover:
  - `fenji_proto::opcode`: `#[non_exhaustive] enum Opcode` with the wire values from Step 3, `from_u8`/`to_u8`, and an `Unknown(u8)` case;
  - `fenji_proto::packet`: `Packet::decode(ty: u8, body: &[u8]) -> Result<Packet>` with `Packet::Unknown { opcode, bytes }` for everything not mapped, plus the first packets the client sends in `WORK` as typed variants if Step 3 maps them;
  - the payload codec (`fenji_proto::pickle` or the format's real name): decode with bounds and typed errors, and encode only if Task 10 needs to send something;
  - the `handshake::Server` change that reaches `Work` (deviation D3), driven by what Step 2 found;
  - the stub's `--speak` change: what it sends after `ok`, and time syncs if Step 5 says they're needed;
  - the scrubbed payload fixture: the auth payload from R1 with the machine-identifying fields (Step 4) replaced, and the test that pins those values;
  - rungs R4 and R5 as Task 10's run steps (R5 on the server-list path, `login.md` §3, with `--gf-sdk=mocked` and the click).

- [ ] **Step 8: Commit and merge.** The amendment is reviewed with the PR.

```bash
scripts/check-forbidden-files.sh
git add docs/protocol docs/superpowers/plans/2026-10-12-m2-transport.md
git commit -m "docs(re): read what follows the client's auth: dispatch, opcodes, payloads" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 10: First packets: opcode and packet decoding, rungs R4–R5

**Files:** as Amendment A lists them.

**Interfaces:**
- Consumes Tasks 6–9, and R3's verdict: (a), or gate G2 decided with a working codec.
- Produces Done-when 3: the real client in `WORK`, its first packets logged decoded, on both login paths. Also `opcodes.md` entries marked `verified`, and the scrubbed payload fixture.

- [ ] **Step 1: Check the preconditions.** R3's verdict is (a), or G2's decision has been implemented. Amendment A is merged. If either is missing, this task waits; record that in `re/m2/progress.md`.

- [ ] **Step 2: Execute Amendment A's steps in order,** TDD as written there, one branch `feat/proto-<issue>-first-packets`.

- [ ] **Step 3: Close out.** R4 and R5 are recorded in `transport.md` §9. `opcodes.md` marks as `verified` the opcodes seen decoded in R4 and R5 that are covered by a fixture test. Then commit, PR and merge as in earlier tasks: `feat(proto): decode the client's first packets after auth`.

---

### Task 11: Oodle helper, MinGW pins, `OodleSpool`

**Files:**
- Modify: `rust-toolchain.toml`, `scripts/versions.env`, `scripts/bootstrap.sh`, `CLAUDE.md` (pinned table), `docs/development.md`, root `Cargo.toml`
- Create: `tools/oodle-helper/Cargo.toml`, `tools/oodle-helper/src/{main,spool,dll}.rs`
- Create: `crates/fenji-data/src/oodle_spool.rs`
- Modify: `crates/fenji-data/src/{lib,blob,error}.rs`, `crates/fenji-data/Cargo.toml`
- Create: `scripts/spike/run-oodle-helper.sh`
- Modify: `.github/workflows/ci.yml` (ShellCheck covers the new script if CI lists scripts explicitly)
- Local only: `re/oodle-spool/`, `re/oodle-prefix/`

**Interfaces:**
- Produces:

```rust
// fenji-data
pub struct blob::Block<'a> { pub index: usize, pub input: &'a [u8], pub raw_len: usize }
pub fn blob::blocks(bytes: &[u8], at: u64) -> Result<(BlobHeader, Vec<Block<'_>>)>;
pub struct oodle_spool::OodleSpool;
impl OodleSpool {
    pub fn new(dir: &Path) -> Result<Self>;                         // creates dir/in and dir/out
    pub fn name(input: &[u8], raw_len: usize) -> String;            // "<sha1 hex of input>-<raw_len>.bin"
    pub fn spool_blob(&self, bytes: &[u8], at: u64) -> Result<usize>; // codec-4 blocks newly queued
}
impl Decompressor for OodleSpool { … }   // codec 4 from out/, others via BuiltinCodecs
Error::OodlePending(String)
// oodle-helper (any OS for the spool logic)
pub fn spool::parse_name(name: &str) -> Option<usize>;
pub fn spool::pending(spool: &Path) -> io::Result<Vec<(String, usize)>>;
pub fn spool::write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()>;
```

- `scripts/spike/run-oodle-helper.sh [--spool DIR]` decodes every pending block, inside the sandbox.

- [ ] **Step 1: Branch and pins** (`chore(deps)` commit). `git switch dev && git pull --ff-only && git switch -c feat/data-<issue>-oodle-helper`
  - `rust-toolchain.toml`: add `targets = ["x86_64-pc-windows-gnu"]`.
  - `scripts/versions.env`: add `MINGW_FEDORA_PACKAGE="mingw64-gcc"   # linker for tools/oodle-helper (x86_64-pc-windows-gnu)` and `MINGW_BREW_PACKAGE="mingw-w64"`.
  - `scripts/bootstrap.sh`: append `${MINGW_FEDORA_PACKAGE}` to `FEDORA_PACKAGES`; in the macOS branch, `brew list --versions "$MINGW_BREW_PACKAGE" >/dev/null 2>&1 || brew install "$MINGW_BREW_PACKAGE"`; and in `--check`, report `x86_64-w64-mingw32-gcc --version`'s first line the way the other tools are reported.
  - `CLAUDE.md` pinned table: a row `| MinGW-w64 | distro package | Fedora mingw64-gcc, Homebrew mingw-w64; linker for tools/oodle-helper (x86_64-pc-windows-gnu target in rust-toolchain.toml) |`.
  - `docs/development.md`: one line under the per-OS setup naming the target and the package.
  - Root `Cargo.toml`: `libloading = "0.9.0"` in `[workspace.dependencies]`, and `sha1.workspace = true` for fenji-data in Step 3.

Run `scripts/bootstrap.sh --check` and `rustup target list --installed | grep windows-gnu`, then commit: `chore(deps): pin the Windows target and MinGW-w64 for the Oodle helper`.

- [ ] **Step 2: `blob::blocks`, test first.** Add to `blob.rs`'s tests:

```rust
    #[test]
    fn blocks_lists_each_block_with_its_raw_length() {
        let content = vec![9u8; 40_000];
        let block = 16_384usize;
        let compressed: Vec<Vec<u8>> = content.chunks(block).map(lz4_flex::block::compress).collect();
        let blob = crate::test_util::lz4_blob_blocks(&compressed, content.len() as u64, block as u32);
        let (h, blocks) = blocks(&blob, 0).unwrap();
        assert_eq!(h.block_sizes.len(), 3);
        let raws: Vec<usize> = blocks.iter().map(|b| b.raw_len).collect();
        assert_eq!(raws, [16_384, 16_384, 40_000 - 2 * 16_384]);
        assert_eq!(blocks[1].input, &compressed[1][..]);
    }
```

`multi_block_lz4` already builds a multi-block blob. Read how, and if `test_util` has no builder taking pre-compressed blocks, add `pub fn lz4_blob_blocks(blocks: &[Vec<u8>], raw_size: u64, block_size: u32) -> Vec<u8>` to `test_util.rs` by moving that test's inline construction into it, unchanged. Then implement:

```rust
/// One block of a blob: its compressed bytes and the raw length it decodes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block<'a> {
    pub index: usize,
    pub input: &'a [u8],
    pub raw_len: usize,
}

/// A blob's header and blocks. A stored blob is one block of its whole data.
pub fn blocks(bytes: &[u8], at: u64) -> Result<(BlobHeader, Vec<Block<'_>>)> {
    let h = BlobHeader::parse(bytes, at)?;
    let data = bytes
        .get(h.header_len()..h.stored_size as usize)
        .ok_or_else(|| Error::BadBlob {
            at,
            reason: format!("{} bytes available, stored size {}", bytes.len(), h.stored_size),
        })?;
    if h.codec == Codec::Stored {
        let one = Block { index: 0, input: data, raw_len: h.raw_size as usize };
        return Ok((h, vec![one]));
    }
    let mut out = Vec::with_capacity(h.block_sizes.len());
    let (mut pos, mut left) = (0usize, h.raw_size);
    for (index, &len) in h.block_sizes.iter().enumerate() {
        let raw_len = left.min(u64::from(h.block_size)) as usize;
        // In range: `data` is exactly the sum of the block sizes (checked by `parse`).
        out.push(Block { index, input: &data[pos..pos + len as usize], raw_len });
        pos += len as usize;
        left -= raw_len as u64;
    }
    Ok((h, out))
}
```

Rewrite `decode` on top of it, keeping its behaviour and its tests:

```rust
pub fn decode(bytes: &[u8], at: u64, codecs: &dyn Decompressor) -> Result<Vec<u8>> {
    let (h, blocks) = blocks(bytes, at)?;
    let mut out = Vec::with_capacity(h.raw_size.min(MAX_PREALLOC) as usize);
    for b in blocks {
        let decoded = codecs.decompress_block(h.codec, b.input, b.raw_len, b.index)?;
        if decoded.len() != b.raw_len {
            return Err(Error::SizeMismatch { got: decoded.len() as u64, want: b.raw_len as u64 });
        }
        out.extend_from_slice(&decoded);
    }
    if out.len() as u64 != h.raw_size {
        return Err(Error::SizeMismatch { got: out.len() as u64, want: h.raw_size });
    }
    Ok(out)
}
```

Run `cargo test -p fenji-data` → expected PASS, the old tests included.

- [ ] **Step 3: `OodleSpool`, test first.** In `crates/fenji-data/Cargo.toml` add `sha1.workspace = true`. In `error.rs`:
  - add `#[error("Oodle block {0} is spooled: run scripts/spike/run-oodle-helper.sh, then retry")] OodlePending(String),`;
  - change `CodecUnavailable`'s message to `"codec {0} is not available here (codec 4 needs an OodleSpool: M2 plan Task 11)"`.

  Then grep the tests for the old wording (`grep -rn "decision gate #39" crates tools`) and update any assertion that quotes it. In `lib.rs`, add `pub mod oodle_spool;`. Create `src/oodle_spool.rs`:

```rust
//! Codec 4 (Oodle LZ) through a spool that `tools/oodle-helper` works on inside the sandbox (M2
//! spec §6; gate #39, route c). A missing block is written to `in/<name>`; the helper writes
//! `out/<name>`. The name binds the input's SHA-1 and the raw length, so an output can only be
//! used for the block it was made from, and only at the right length.
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};

use crate::blob::blocks;
use crate::{BuiltinCodecs, Codec, Decompressor, Error, Result};

pub struct OodleSpool {
    dir: PathBuf,
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

impl OodleSpool {
    pub fn new(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir.join("in"))?;
        fs::create_dir_all(dir.join("out"))?;
        Ok(Self { dir: dir.to_path_buf() })
    }

    pub fn name(input: &[u8], raw_len: usize) -> String {
        let digest = Sha1::digest(input);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        format!("{hex}-{raw_len}.bin")
    }

    /// Queues every codec-4 block of the blob at `bytes` that has no output yet. Returns how many
    /// it newly queued.
    pub fn spool_blob(&self, bytes: &[u8], at: u64) -> Result<usize> {
        let (h, list) = blocks(bytes, at)?;
        if h.codec != Codec::Oodle {
            return Ok(0);
        }
        let mut queued = 0;
        for b in list {
            let name = Self::name(b.input, b.raw_len);
            let (inp, out) = (self.dir.join("in").join(&name), self.dir.join("out").join(&name));
            if !out.exists() && !inp.exists() {
                write_atomic(&inp, b.input)?;
                queued += 1;
            }
        }
        Ok(queued)
    }
}

impl Decompressor for OodleSpool {
    fn decompress_block(&self, codec: Codec, input: &[u8], raw_len: usize, index: usize) -> Result<Vec<u8>> {
        if codec != Codec::Oodle {
            return BuiltinCodecs.decompress_block(codec, input, raw_len, index);
        }
        let name = Self::name(input, raw_len);
        match fs::read(self.dir.join("out").join(&name)) {
            Ok(bytes) if bytes.len() == raw_len => Ok(bytes),
            Ok(bytes) => Err(Error::SizeMismatch { got: bytes.len() as u64, want: raw_len as u64 }),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let inp = self.dir.join("in").join(&name);
                if !inp.exists() {
                    write_atomic(&inp, input)?;
                }
                Err(Error::OodlePending(name))
            }
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spool() -> (tempfile::TempDir, OodleSpool) {
        let tmp = tempfile::tempdir().unwrap();
        let s = OodleSpool::new(tmp.path()).unwrap();
        (tmp, s)
    }

    #[test]
    fn name_binds_raw_len() {
        assert_ne!(OodleSpool::name(b"abc", 10), OodleSpool::name(b"abc", 11));
        assert_eq!(
            OodleSpool::name(b"abc", 10),
            "a9993e364706816aba3e25717850c26c9cd0d89d-10.bin"
        );
    }

    #[test]
    fn miss_spools_then_hit_reads_output() {
        let (tmp, s) = spool();
        let err = s.decompress_block(Codec::Oodle, b"packed", 4, 0).unwrap_err();
        let Error::OodlePending(name) = err else { panic!("{err:?}") };
        assert_eq!(fs::read(tmp.path().join("in").join(&name)).unwrap(), b"packed");
        fs::write(tmp.path().join("out").join(&name), b"raw!").unwrap();
        assert_eq!(s.decompress_block(Codec::Oodle, b"packed", 4, 0).unwrap(), b"raw!");
    }

    #[test]
    fn short_output_is_not_accepted() {
        let (tmp, s) = spool();
        let name = OodleSpool::name(b"packed", 4);
        fs::write(tmp.path().join("out").join(&name), b"raw").unwrap();
        assert!(matches!(
            s.decompress_block(Codec::Oodle, b"packed", 4, 0),
            Err(Error::SizeMismatch { got: 3, want: 4 })
        ));
    }

    #[test]
    fn pending_input_is_written_once() {
        let (tmp, s) = spool();
        let name = OodleSpool::name(b"packed", 4);
        let _ = s.decompress_block(Codec::Oodle, b"packed", 4, 0);
        let p = tmp.path().join("in").join(&name);
        let before = fs::metadata(&p).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _ = s.decompress_block(Codec::Oodle, b"packed", 4, 0);
        assert_eq!(fs::metadata(&p).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn other_codecs_pass_through() {
        let (_tmp, s) = spool();
        assert_eq!(s.decompress_block(Codec::Stored, b"plain", 5, 0).unwrap(), b"plain");
    }
}
```

`crates/fenji-data/Cargo.toml` already has `tempfile` as a dev-dependency. Run `cargo test -p fenji-data oodle_spool` → expected PASS, 5 tests. Write the tests first and watch them fail to compile, then add the implementation.

- [ ] **Step 4: The helper crate.** Add `"tools/oodle-helper"` to the workspace members.

`tools/oodle-helper/Cargo.toml`:

```toml
[package]
name = "fenji-oodle-helper"
description = "Decodes spooled Oodle LZ blocks with the client's own oo2core_6_win64.dll, under Wine inside the sandbox only (M2 spec §6, gate #39 route c)"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[[bin]]
name = "oodle-helper"
path = "src/main.rs"

[target.'cfg(windows)'.dependencies]
libloading.workspace = true

[dev-dependencies]
tempfile.workspace = true

# Deviation D1: not the workspace's `forbid`, so that dll.rs alone may allow it.
[lints.rust]
unsafe_code = "deny"

[lints.clippy]
all = { level = "warn", priority = -1 }
```

`src/spool.rs` (platform-independent, tested on Linux in CI):

```rust
//! The spool's file names and atomic writes; the same layout as fenji-data's OodleSpool.
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// The most a block may decode to (the archives' blocks are 512 KiB).
pub const MAX_RAW: usize = 64 << 20;

/// `<40 hex>-<raw length>.bin` → the raw length.
pub fn parse_name(name: &str) -> Option<usize> {
    let stem = name.strip_suffix(".bin")?;
    let (hex, len) = stem.split_once('-')?;
    if hex.len() != 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return None;
    }
    if len.is_empty() || !len.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    len.parse().ok().filter(|&n| n <= MAX_RAW)
}

/// Inputs in `spool/in` without an output in `spool/out`, sorted by name.
pub fn pending(spool: &Path) -> io::Result<Vec<(String, usize)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(spool.join("in"))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if let Some(raw) = parse_name(&name) {
            if !spool.join("out").join(&name).exists() {
                out.push((name, raw));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Writes `dir/name` through a temporary file, so a reader never sees a partial output.
pub fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

    #[test]
    fn names() {
        assert_eq!(parse_name(&format!("{H}-524288.bin")), Some(524_288));
        assert_eq!(parse_name(&format!("{H}-0.bin")), Some(0));
        for bad in [
            format!("{H}-1.tmp"),
            format!("{H}.bin"),
            format!("{}-1.bin", H.to_uppercase()),
            format!("{H}-x.bin"),
            format!("{H}--1.bin"),
            format!("{H}-{}.bin", MAX_RAW + 1),
            "../x-1.bin".to_string(),
        ] {
            assert_eq!(parse_name(&bad), None, "{bad}");
        }
    }

    #[test]
    fn pending_skips_done_and_foreign_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("in")).unwrap();
        fs::create_dir_all(tmp.path().join("out")).unwrap();
        let a = format!("{H}-1.bin");
        let b = format!("{}-2.bin", "b".repeat(40));
        fs::write(tmp.path().join("in").join(&a), b"x").unwrap();
        fs::write(tmp.path().join("in").join(&b), b"y").unwrap();
        fs::write(tmp.path().join("in").join("notes.txt"), b"z").unwrap();
        write_atomic(&tmp.path().join("out"), &a, b"1").unwrap();
        assert_eq!(pending(tmp.path()).unwrap(), [(b, 2)]);
        assert!(!tmp.path().join("out").join(format!(".{a}.tmp")).exists());
    }
}
```

`src/dll.rs` (Windows only):

```rust
//! The one place this workspace calls foreign code: OodleLZ_Decompress in the client's own
//! oo2core_6_win64.dll, with the arguments the client itself passes (FUN_14024E060; M2 plan F8).
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::path::Path;
use std::ptr::null_mut;

use libloading::Library;

/// 14 arguments, as at the client's call site: input, input length, output, raw length, then the
/// constants 1, 0, 0, null, 0, null, null, null, 0, 3.
type Decompress = unsafe extern "C" fn(
    *const u8, isize, *mut u8, isize,
    i32, i32, i32,
    *mut u8, isize, *mut c_void, *mut c_void, *mut c_void, isize,
    i32,
) -> isize;

pub struct Oodle {
    _lib: Library,
    decompress: Decompress,
}

impl Oodle {
    pub fn load(path: &Path) -> Result<Self, String> {
        // SAFETY: loading runs the DLL's initialisation. This helper exists to load exactly this
        // DLL from the owner's client copy, and it runs only inside the sandbox (run-oodle-helper.sh).
        let lib = unsafe { Library::new(path) }.map_err(|e| format!("{}: {e}", path.display()))?;
        // SAFETY: the symbol's type is the one the client's own call site uses (F8).
        let decompress = unsafe { lib.get::<Decompress>(b"OodleLZ_Decompress\0") }
            .map(|s| *s)
            .map_err(|e| format!("OodleLZ_Decompress: {e}"))?;
        Ok(Self { _lib: lib, decompress })
    }

    /// The raw bytes, or `None` if the DLL doesn't return exactly `raw_len` bytes.
    pub fn decompress(&self, input: &[u8], raw_len: usize) -> Option<Vec<u8>> {
        let mut out = vec![0u8; raw_len];
        // SAFETY: both buffers are valid for the lengths passed; the library outlives the call.
        let n = unsafe {
            (self.decompress)(
                input.as_ptr(), input.len() as isize, out.as_mut_ptr(), raw_len as isize,
                1, 0, 0,
                null_mut(), 0, null_mut(), null_mut(), null_mut(), 0,
                3,
            )
        };
        (n == raw_len as isize).then_some(out)
    }
}
```

`src/main.rs`:

```rust
//! oodle-helper DLL SPOOL: decodes every pending block in SPOOL/in into SPOOL/out with the client's
//! own Oodle DLL. Windows target only; runs under Wine through scripts/spike/run-oodle-helper.sh.
mod spool;
#[cfg(windows)]
mod dll;

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, dll_path, spool_dir] = args.as_slice() else {
        eprintln!("usage: oodle-helper DLL SPOOL");
        return ExitCode::from(2);
    };
    let spool_dir = std::path::Path::new(spool_dir);
    let oodle = match dll::Oodle::load(std::path::Path::new(dll_path)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let jobs = match spool::pending(spool_dir) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("{}: {e}", spool_dir.display());
            return ExitCode::from(1);
        }
    };
    let (mut done, mut failed) = (0usize, 0usize);
    for (name, raw_len) in &jobs {
        let input = match std::fs::read(spool_dir.join("in").join(name)) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{name}: {e}");
                failed += 1;
                continue;
            }
        };
        match oodle.decompress(&input, *raw_len) {
            Some(raw) => match spool::write_atomic(&spool_dir.join("out"), name, &raw) {
                Ok(()) => done += 1,
                Err(e) => {
                    eprintln!("{name}: {e}");
                    failed += 1;
                }
            },
            None => {
                eprintln!("{name}: the DLL did not return {raw_len} bytes");
                failed += 1;
            }
        }
    }
    println!("{done} decoded, {failed} failed, {} pending before", jobs.len());
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) }
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("oodle-helper is a Windows program: build it with --target x86_64-pc-windows-gnu and run it through scripts/spike/run-oodle-helper.sh");
    ExitCode::from(2)
}
```

Add `pub` re-exports only as clippy needs. On Linux, `spool.rs` is reached only from the tests and the Windows `main`; to stop dead-code warnings there, put `#![cfg_attr(not(windows), allow(dead_code))]` at the top of `main.rs`.

Run:
```bash
cargo test -p fenji-oodle-helper
cargo build --release -p fenji-oodle-helper --target x86_64-pc-windows-gnu
cargo clippy -p fenji-oodle-helper --target x86_64-pc-windows-gnu -- -D warnings
```
Expected: 2 tests pass; `target/x86_64-pc-windows-gnu/release/oodle-helper.exe` exists; clippy is clean. If the GNU linker isn't found, check `x86_64-w64-mingw32-gcc --version` (Step 1).

- [ ] **Step 5: `scripts/spike/run-oodle-helper.sh`:**

```bash
#!/usr/bin/env bash
# Runs tools/oodle-helper under GE-Proton's Wine inside scripts/spike/isolated-run.sh, to decode the
# Oodle blocks fenji-data spooled (M2 spec §6; gate #39 route c, dev analysis on the owner's machines
# only). The client's own oo2core_6_win64.dll is loaded read-only from $FENJI_CLIENT_PATH/bin64;
# gujianol.exe is never started. No network but loopback; non-root.
#
#   scripts/spike/run-oodle-helper.sh [--spool DIR]     (default DIR: re/oodle-spool)
#
# Exit: the helper's status (0 all decoded, 1 some failed, 2 usage); 1 or 2 for setup errors here.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
REPO=$(cd -- "$HERE/../.." && pwd -P)
# shellcheck source=../versions.env
. "$REPO/scripts/versions.env"
PROTON="${FENJI_GE_PROTON:-$HOME/.local/share/fenji/tools/$GE_PROTON_VERSION}"
CLIENT="${FENJI_CLIENT_PATH:-$HOME/fenji-client}"
HELPER="$REPO/target/x86_64-pc-windows-gnu/release/oodle-helper.exe"
PREFIX="$REPO/re/oodle-prefix"
SPOOL="$REPO/re/oodle-spool"
die() { echo "run-oodle-helper: $*" >&2; exit 1; }
while [ $# -gt 0 ]; do
  case "$1" in
    --spool) [ $# -ge 2 ] || die "--spool needs a directory"; SPOOL=$(realpath -m -- "$2"); shift 2 ;;
    *) echo "usage: $0 [--spool DIR]" >&2; exit 2 ;;
  esac
done
case "$SPOOL/" in "$REPO/re/"*) ;; *) die "the spool must be under re/: $SPOOL" ;; esac
[ -x "$PROTON/files/bin/wine" ] || die "GE-Proton not found at $PROTON (scripts/bootstrap.sh)"
[ -f "$HELPER" ] || die "build it first: cargo build --release -p fenji-oodle-helper --target x86_64-pc-windows-gnu"
DLL="$CLIENT/bin64/oo2core_6_win64.dll"
[ -f "$DLL" ] || die "no $DLL"
mkdir -p "$PREFIX" "$SPOOL/in" "$SPOOL/out"
exec "$HERE/isolated-run.sh" --no-root \
  --ro-bind "$PROTON" --ro-bind "$CLIENT/bin64" --ro-bind "$(dirname -- "$HELPER")" \
  --bind "$PREFIX" --bind "$SPOOL" -- \
  env WINEPREFIX="$PREFIX" WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml=" \
  "$PROTON/files/bin/wine" "$HELPER" "Z:$DLL" "Z:$SPOOL"
```

`chmod +x` it. Wine maps `Z:` to `/`. Wine's path conversion accepts `Z:/home/...` with forward slashes. If it doesn't, convert with `"Z:${DLL//\//\\}"`.

Run `shellcheck scripts/spike/run-oodle-helper.sh` → clean. If `.github/workflows/ci.yml` lists the shell scripts for ShellCheck by name, add this one.

- [ ] **Step 6: The first real decode** (a helper run counts as a client-side run: clean session; on the Fedora box).
  1. Spool `_index`'s blocks with a throwaway test or example, without committing it. Read `$FENJI_CLIENT_PATH/data/_index` fully and call `OodleSpool::new(Path::new("re/oodle-spool"))?.spool_blob(&bytes, 0)`. Expected: 49 queued.
  2. Run `scripts/spike/run-oodle-helper.sh`. Expected: `49 decoded, 0 failed`.
  3. If the DLL returns the wrong length for every block, the archives' blocks may carry a `u32` raw-length prefix the way F8's wrapper does, or none while that wrapper adds 4. Find which wrapper the archive reader uses: FUN_14024E060 or one of spike-0 4.6's others, through the blob reader that M1 read, `m1-archives.md` G-keys. Pass the input to match. Record the answer in `docs/re/m1-archives.md` §3 as an "M2 update" note.
  4. Check that the 49 outputs joined are text lines `<40 hex>\t<path>`, by looking at the first 3 lines locally only.

- [ ] **Step 7: Gates and commit** (one PR, two commits: Step 1's `chore(deps)`, then this):

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo deny check
scripts/check-forbidden-files.sh
git add tools/oodle-helper crates/fenji-data scripts/spike/run-oodle-helper.sh Cargo.toml Cargo.lock .github/workflows/ci.yml docs/re/m1-archives.md
git commit -m "feat(data): decode Oodle blocks with the client's DLL in a sandboxed helper" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 12: `_index` decoded and `archive paths`

**Files:**
- Create: `crates/fenji-data/src/index.rs`
- Modify: `crates/fenji-data/src/lib.rs`, `tools/archive/src/main.rs`, `tools/archive/tests/cli.rs`, `docs/re/m1-archives.md` (an "M2 update" note)

**Interfaces:**
- Consumes Task 11's `OodleSpool` and `blob::decode`.
- Produces:

```rust
pub enum index::Line<'a> { Add { key: [u8; 20], path: &'a str }, Remove { path: &'a str } }
pub fn index::parse_line(line: &str) -> Option<Line<'_>>;
pub struct index::PathMap { pub by_path: BTreeMap<String, [u8; 20]>, pub containers: BTreeSet<String>, pub bad_lines: usize }
pub fn index::build(text: &str) -> PathMap;
```

- `archive paths [--spool DIR] --out DIR` writes `DIR/paths.tsv` (`key path part offset size codec`) under `re/`.

- [ ] **Step 1: Branch, then write the failing parser tests** in `index.rs`. The rules come from `m1-archives.md` 3.2: lines of `<40 hex>\t<path>`; a leading `-` removes a path; a path ending in `$$` is a container.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const K: &str = "00112233445566778899aabbccddeeff00112233";

    #[test]
    fn parses_adds_removes_and_containers() {
        let text = format!(
            "{K}\tdata/tables/item.tab\n{K}\tdata/maps/a.zip$$\n-data/old.tab\nnot a line\n\n{K}\tdata/old.tab\n"
        );
        let m = build(&text);
        assert_eq!(m.by_path.len(), 3);
        assert!(m.by_path.contains_key("data/tables/item.tab"));
        assert!(m.containers.contains("data/maps/a.zip$$"));
        assert_eq!(m.bad_lines, 1);
    }

    #[test]
    fn a_later_remove_drops_the_path() {
        let text = format!("{K}\tdata/x.tab\n-data/x.tab\n");
        assert!(build(&text).by_path.is_empty());
    }

    #[test]
    fn key_must_be_40_lowercase_hex() {
        assert!(parse_line(&format!("{}\tp", &K[..39])).is_none());
        assert!(parse_line(&format!("{}\tp", K.to_uppercase())).is_none());
        assert!(parse_line(&format!("{K}\t")).is_none());
        assert!(matches!(parse_line(&format!("{K}\tp")), Some(Line::Add { path: "p", .. })));
    }

    #[test]
    fn crlf_is_tolerated() {
        let m = build(&format!("{K}\tdata/a.tab\r\n"));
        assert!(m.by_path.contains_key("data/a.tab"));
    }
}
```

Run `cargo test -p fenji-data index` → expected FAIL.

- [ ] **Step 2: Implement `index.rs`:**

```rust
//! `_index`'s text: `<40 hex>\t<path>` lines (docs/re/m1-archives.md 3.2). A line starting with `-`
//! removes a path; a path ending in `$$` is a container. Uppercase hex is refused, as the engine's
//! keys are lowercase in every line seen (Task 11, Step 6).
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line<'a> {
    Add { key: [u8; 20], path: &'a str },
    Remove { path: &'a str },
}

fn unhex(s: &str) -> Option<[u8; 20]> {
    if s.len() != 40 || !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    let mut out = [0u8; 20];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

pub fn parse_line(line: &str) -> Option<Line<'_>> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    if let Some(path) = line.strip_prefix('-') {
        return (!path.is_empty()).then_some(Line::Remove { path });
    }
    let (hex, path) = line.split_once('\t')?;
    if path.is_empty() {
        return None;
    }
    Some(Line::Add { key: unhex(hex)?, path })
}

#[derive(Debug, Default)]
pub struct PathMap {
    pub by_path: BTreeMap<String, [u8; 20]>,
    pub containers: BTreeSet<String>,
    pub bad_lines: usize,
}

pub fn build(text: &str) -> PathMap {
    let mut m = PathMap::default();
    for line in text.split('\n').filter(|l| !l.trim().is_empty()) {
        match parse_line(line) {
            Some(Line::Add { key, path }) => {
                if path.ends_with("$$") {
                    m.containers.insert(path.to_string());
                }
                m.by_path.insert(path.to_string(), key);
            }
            Some(Line::Remove { path }) => {
                m.by_path.remove(path);
                m.containers.remove(path);
            }
            None => m.bad_lines += 1,
        }
    }
    m
}
```

Add `pub mod index;` to `lib.rs`. Run → PASS, 4 tests.

- [ ] **Step 3: `archive paths`.** In `tools/archive/src/main.rs`:
  - Add the subcommand `Paths { #[arg(long, default_value = "re/oodle-spool")] spool: PathBuf, #[arg(long)] out: PathBuf }`, documented "Decode _index through the Oodle spool and write OUT/paths.tsv (key, path, part, offset, size, codec); OUT must be inside re/".
  - Resolve `out` with `fenji_data::re_dir::resolve_out_dir` as `extract` does.
  - Read `data/_index`, and call `OodleSpool::spool_blob`. If it queued any block, or `blob::decode` returns `Error::OodlePending`, print `N blocks spooled: run scripts/spike/run-oodle-helper.sh, then re-run` and exit with status 3.
  - Otherwise decode, `index::build` the text, and index every live record of every part by key: reuse `info`'s part loop (`read_part_header`, `h.live()`).
  - Write one TSV row per path, with `-` for the part columns when no live record has the key.
  - Print a summary to stdout: paths, containers, bad lines, paths found in a part, paths not found.

  Add a CLI test in `tools/archive/tests/cli.rs` that runs `archive paths --out /tmp/x` and expects the existing "inside re/" refusal; follow the file's existing refusal test.

- [ ] **Step 4: The real run** (Fedora box, after Task 11): `cargo run --release -p fenji-archive -- paths --out re/m2/archive`. Expected: the summary, with about 224,000 paths, most of them found. Record the numbers in a short "M2 update" note at the end of `docs/re/m1-archives.md` §3: counts only, no paths.

- [ ] **Step 5: Gates and commit.**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked
scripts/check-forbidden-files.sh
git add crates/fenji-data tools/archive docs/re/m1-archives.md
git commit -m "feat(data): decode _index and map paths to archive records" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin HEAD && gh pr create --base dev --fill
gh pr checks --watch && gh pr merge --squash --delete-branch
```

---

### Task 13: Table format and `archive dump`

**Files:**
- Create: `docs/re/m2-tables.md`
- Modify: this plan (append "Amendment B: table parsing and `archive dump`"), then the files Amendment B names
- Local only: `re/m2/tables/`

**Interfaces:**
- Consumes `re/m2/archive/paths.tsv` and `OodleSpool`.
- Produces Done-when 5: `archive dump` for items, skills, monsters, the XP curve and maps, with output only under `re/`. Closes #51.

The RE part (Steps 1–3) needs a clean session.

- [ ] **Step 1: Find the tables.**
  - From `paths.tsv`, list the table-like files: `.tab`, and any other extension under a `tables`-like directory.
  - Pick the candidates for items, skills, monsters, the XP curve and maps by path name.
  - Decode those files: `archive extract --key <hex> --out re/m2/tables`. If `extract` can't take one key, add `--key` to it in Amendment B's code and use a throwaway test for now.

- [ ] **Step 2: Read the `.tab` format.**
  - Find the engine's reader in Ghidra: search for the extension string and the loader's callers, and the Lua scripts that read tables in `re/m1/disasm`.
  - Record the header, the column types, the string storage and the row layout, tagged as usual.
  - Check it against two decoded files.

- [ ] **Step 3: Write `docs/re/m2-tables.md`:** the format (prose and tables), the five tables' paths (by directory, not content), their columns, and the open questions. Add a row to `docs/README.md`:

```markdown
| [`re/m2-tables.md`](re/m2-tables.md) | M2: the client's table format, and where items, skills, monsters, the XP curve and maps live. |
```

- [ ] **Step 4: Write Amendment B** at the end of this plan, under the heading `## Amendment B: table parsing and archive dump (from Task 13)`, in this plan's format. It covers:
  - `fenji_data::table`: a typed parser with tests on synthetic tables built by the tests;
  - `archive dump --table <items|skills|monsters|xp|maps> --out DIR`: TSV or JSON lines under `re/`;
  - the local-only integration test gated on `FENJI_CLIENT_PATH`.

- [ ] **Step 5: Execute Amendment B** with TDD, as written, on the same branch.

- [ ] **Step 6: Commit and merge** with `Closes #<issue>` and `Closes #51`: `feat(data): read the client's tables and dump them`.

---

### Task 14: M2 close-out: records, spec notes, final review, v0.3.0

**Files:**
- Create: `docs/re/m2-summary.md`
- Modify:
  - `docs/superpowers/specs/2026-10-11-fenji-design.md` (§6.2, §10, §12)
  - `docs/superpowers/specs/2026-10-12-m2-transport-design.md` (status line)
  - `docs/README.md`
  - `docs/re/sources.md` (only if an outside source was used)
  - `CLAUDE.md` (Status, at release)
- Generated: `CHANGELOG.md` (at release)

- [ ] **Step 1: Write `docs/re/m2-summary.md`.** Branch `docs/re-<issue>-m2-close`. It covers:
  - what M2 delivered, in a short paragraph;
  - a table of the spike-0 and M1 open questions M2 answered (spike-0 4.7, 4.9, 4.14, 4.23–4.28, 4.30; login.md §5's first two; 1.8), each with its answer and the doc that holds it;
  - the deviations D1–D4 and how each ended;
  - the Done-when list (spec §10), with each item's status and evidence;
  - the gates hit (G1, G2) and the owner's rulings.

  Add the `docs/README.md` row:

```markdown
| [`re/m2-summary.md`](re/m2-summary.md) | M2 summary: the transport answered, deviations, Done-when status. |
```

- [ ] **Step 2: Add "M2 update" notes to the parent spec.**
  - §6.2: the real framing (F3–F6) replaces the "u32 length prefix" and "rolling XOR" bullets, and the Oodle Network outcome.
  - §10: the M2 row's outcome.
  - §12: an "Answer (M2)" column for the Oodle Network row, the handshake row and the one-or-two-connections row; the others get "—".
  - Set the M2 spec's status line to `implemented in M2 (v0.3.0); see docs/re/m2-summary.md`.
  - Commit `docs(re): summarise M2 and update the spec`, and open a PR. **Hold the merge until Step 3's review is clean.**

- [ ] **Step 3: Final whole-branch review.**
  - Run `git diff origin/main origin/dev > re/m2/final-review.diff`.
  - Dispatch one fresh reviewer (`general-purpose`, the most capable model) that has never read rejected material. Its brief:
    - the M2 spec and this plan, with its amendments;
    - the Global Constraints and Review Focus;
    - the diff;
    - Done-when 1–7, checked against the evidence;
    - "triage every deferred note in `re/m2/progress.md`".
  - Make one fix wave, then one scoped re-review. Merge Step 2's PR when it is clean.

- [ ] **Step 4: Release v0.3.0, only after the owner says go.** Follow `docs/development.md`'s release procedure exactly, with tag `v0.3.0` and milestone "M2 Transport + handshake":
  - stage `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md` and `CLAUDE.md` explicitly; the Status line says M2 is released and M3 is next;
  - check `git diff --cached --stat`;
  - the release notes name the pinned build;
  - close the release issue and the milestone.
