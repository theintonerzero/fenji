# M1: Data layer — Design Spec

**Status:** approved by the owner on 2026-10-12. Amended the same day by the plan's deviations D1–D3, which the owner accepted ([plan](../plans/2026-10-12-m1-data-layer.md), "Deviations from the spec").
**Parent:** [Fenji design spec](2026-10-11-fenji-design.md) §6.3 and §10 (M1), and the M0 findings in [`docs/re/spike-0.md`](../../re/spike-0.md).

## 1. Purpose and priority

M1 makes the game's own scripts and data readable to us. The owner chose to optimise for **the fastest route to the real client connecting to our stub**: the Lua track comes first, and M1 ends with a sandboxed run in which the client connects. The archive track runs alongside it, gated on the Oodle decision.

The spike shapes the order:
- The embedded Lua is stored as **LZ4** blocks (spike-0 6.12–6.14). Reading it needs no Oodle and no gate.
- The archives and `_index` look **Oodle-Kraken**-compressed (6.5–6.9). Reading the tables needs the Oodle decision, which is a ground-rule-6 gate.

M1 builds analysis tooling only. It ships no server and no launcher, and never changes the client. Everything it extracts or decompiles stays in the git-ignored `re/`.

## 2. Workstreams

| # | Workstream | Delivers | Depends on |
|---|---|---|---|
| W1 | Lua front half | `fenji-lua`: locate chunks, undo the LZ4, parse the modified 5.3 format, disassemble. `luatool list`, `extract` and `disasm` | A clean-agent Ghidra task: the dropped or merged header field (6.16), the opcode numbering and the meaning of the 7 extra opcodes (6.17–6.19), and how chunks are indexed |
| W2 | Readable source | the normaliser to stock Lua 5.3, plus `luatool decompile` through an external, clearly licensed decompiler (§3) | W1 |
| W3 | Login-stage hunt | `docs/protocol/login.md`: what gives the client its server address, and why its UI is German if the same scripts show it | W2 |
| W4 | Client runner (#17) | a tracked runner and AGT watchdog with tests; GE-Proton pinned | — |
| W5 | First connection | a sandboxed run in which the real client connects to `fenji-stub` | W3, W4 |
| W6 | Server-logic measurement | `docs/re/m1-script-census.md` | W2 |
| W7 | Archive track | `fenji-data` parsing without decompression; the `archive` tool; the Oodle decision-gate issue; table dumps after the decision | W7's gate issue opens early; dumps wait on the owner |

The critical path is W1 → W2 → W3 → W5. W4 and W7 start at once, and W6 follows W2.

## 3. `fenji-lua` and `luatool`

### Approach

Normalise, then decompile. `fenji-lua` turns the client's dialect into stock Lua 5.3 bytecode, and an existing, clearly licensed Lua 5.3 decompiler turns that into source.
- The likely candidate is unluac, which is Java; Java 21 is already pinned for Ghidra.
- Its licence is verified and recorded in `docs/re/sources.md` before any use.
- It runs only as an external tool, never linked, with its output only in `re/`.
- Our own disassembly stays the ground truth wherever the decompiler is wrong or fails.

Rejected:
- **Our own decompiler in Rust:** a large project that nothing in M1–M2 needs beyond readability. The M5 VM runs bytecode, not source.
- **Disassembly only:** too slow for 18,587 chunks. It remains the fallback if the decompiler doesn't work out.

### Units (`crates/fenji-lua`)

> **Amended (D1, D2; owner, 2026-10-12):** the embedded scripts turned out to be files in the engine's blob format, which the archives share. So locating and decompressing live in `fenji-data` (`exe_fs`, `blob`, `codec`), and chunks are found through the client's own registry, not by scanning. `fenji-lua` keeps `chunk`, `disasm` and `normalize`, and adds `census`.

- **`locate`:** finds every embedded chunk in `gujianol.exe` and returns its offset, compressed size and script path where known.
  - The preferred source is the client's own chunk registry, if W1's Ghidra task finds one.
  - The fallback is scanning for the `F6 04` + LZ4 pattern (6.12–6.13) and decoding until a chunk parses completely.
- **`lz4`:** block decompression with `lz4_flex` (MIT/Apache-2.0).
- **`chunk`:** parses chunk bytes into a typed function tree: header, constants, upvalues, instructions, nested functions and debug info.
  - Format differences live in a `Dialect` value (header layout, opcode table). Stock Lua 5.3 is one dialect and the client's is another.
  - Unknown opcodes decode to `Op::Unknown(n)`, and malformed input returns an error; nothing panics.
- **`disasm`:** the function tree → a text listing.
- **`normalize`:** the client's dialect → stock 5.3 bytes. The 7 extra opcodes map to stock equivalents where their meaning allows, and otherwise to marked placeholders. It rests on a writer whose round-trip (parse stock → write → identical bytes) is a test.

### `luatool` (`tools/luatool`)

- **Subcommands:** `list`, `extract`, `disasm`, `decompile`. Each takes one chunk, a path prefix (e.g. `login/`) or all chunks.
- **The decompiler** is located through a `versions.env` entry with its version and SHA-256, installed by bootstrap.
- **Output goes only under `re/`;** the tool refuses any other output directory.
- **Failures:** per-chunk failures are counted and reported at the end, and one bad chunk never stops a batch. `decompile` reports a success rate per path area.

### Testing without game bytes in git

- **Fixtures** are our own small Lua sources compiled with stock `luac` 5.3. That is our code, not game content.
- **A synthetic "client dialect"** is built from those fixtures by the test writer, so `locate`, `lz4`, `chunk`, `normalize` and `disasm` are all exercised in CI.
- **A local-only integration test**, `#[ignore]`d in CI and run when `FENJI_CLIENT_PATH` is set, checks the real numbers: 18,587 chunks decode and parse, and disassembly finishes.

## 4. Login hunt, runner, first connection, measurement

### W3: login-stage hunt

Clean agents trace from what M0 observed to the code that decides where the client connects. M0 observed:
- the "Load Login Scene" and "FAILED TO FETCH GAMEFORGE SERVER LIST!" log lines;
- the probe for a `servername` file at the install root;
- the `--server-name`, `--server_locale` and `--net` flags (spike-0 8.15–8.17, 8.32, 8.37).

The write-up is `docs/protocol/login.md`:
- script paths, function names, constants and file formats, tagged Observed/Inferred/Unknown;
- no script text;
- the Ghidra addresses or REA Evidence IDs it relies on.

If the only way to redirect the client is to patch it, or anything else under ground rule 6, work stops and a decision-gate issue goes to the owner.

### W4: client runner (#17)

- **The runner and watchdog** become tracked files, `scripts/spike/run-client.sh` and a stdlib-Python watchdog, replacing the local `re/spike` scripts. They get the same review standard as `isolated-run.sh`.
- **The watchdog's rules get CI tests** on synthetic logs and directories. Two known false positives must stay quiet:
  - Proton re-creating its own built-in driver links during a prefix re-sync (the owner's look-1, 2026-10-12);
  - Wine's split `winebth` log line (run 7).

  A new or genuinely changed driver file must still stop the run. Links count as changed only if their target or the target's content changes.
- **GE-Proton** gets its version and SHA-256 pinned in `scripts/versions.env`, is installed by `scripts/bootstrap.sh` (Fedora), and is added to CLAUDE.md's pinned table in the same `chore(deps)` change.

### W5: first connection

- **Runs:** at most 4. W3's input points the client at `fenji-stub` through the hosts file or a local file, inside `isolated-run.sh --no-root --x11 N --cpu-topology`, with `--gf-sdk=mocked` (dev-only, gate #11) and `--fs=data:_index:_index`.
- **Done when** the stub logs a session from the real client. "Connected, 0 bytes" passes, because spike-0 4.23–4.28 reads the server as speaking first. Answering the hello is M2's work.
- **Low ports:** ports below 1024 can't be bound under `--no-root`. If the client needs one, the choice goes to the owner.

### W6: server-logic measurement

Built from the disassembly, not by reading source, per script area (`combat/skill`, `combat/buff`, `combat/effect`, `combat/server`, …):
- chunk, function and instruction counts;
- the share of instructions in code that runs only after an `IsServer()` check, and the same for `IsClient()`;
- areas with no client-side counterpart.

The output is `docs/re/m1-script-census.md`, numbers only. It feeds M5's per-system decision between scripts and Rust (parent spec §6.4).

## 5. Archive track (`fenji-data`, `tools/archive`) and the Oodle gate

### Units without decompression

- **`part`:** the `ZFS\0[IX]` header, the 4-byte field and the 4,096-record table of each `data0NN` part. Unknown fields stay raw rather than guessed.
  - Spike-0 found the records tile the file in only 10 of 100 parts (6.3), so a clean-agent Ghidra task reads the engine's own archive reader first.
- **`blob`:** the 32-byte entry header: compression type (0 stored, 4 Oodle), raw and stored sizes, timestamp, chunk size.
- **`index`:** the header of `_index`. Its body is one compressed blob in 512 KiB chunks (6.8–6.9), unreadable until the gate is decided.
- **A `Decompressor` trait,** with a `Stored` implementation now. The gate's outcome plugs in behind it.
- **`archive` CLI:** `info`, `list` and `extract --stored-only`, with output only under `re/`. Tests use synthetic parts built by the tests, plus a local-only check against the owner's install.

### The Oodle decision gate

The gate issue opens early in M1. It covers what we found and the options, each with legal and technical risk, effort and a recommendation:
- **(a) A clean-room Kraken decoder in Rust:** large.
- **(b) A clearly licensed open-source decoder used only as an external tool:** output stays in `re/`, and the licence is verified first.
- **(c) Calling the client's own `oo2core_6_win64.dll` in a sandboxed helper:** this is the middleware-licence gate.
- **(d) Deferring tables beyond M1.**

Table parsing and the `archive dump` command wait for the owner's decision. If the chosen route is slow, the owner decides at the gate whether table dumps leave M1's Done-when.

## 6. Process

- One plan: `docs/superpowers/plans/2026-10-12-m1-data-layer.md`. Each task becomes an issue in milestone M1.
- Execution is subagent-driven: an implementer per task, a task review, fix rounds, and a final whole-branch review.
- Rulings made without the owner go in the ledger for ratification. Decision gates stop the affected work and go to the owner as issues.
- **Clean-room hygiene (CLAUDE.md):** Ghidra work, reading disassembly or decompiled output, and every client run go only to agents that have never read rejected material. Writing the parsers from their documented findings is ordinary code work.
- **Evidence:** pinned-exe addresses (Ghidra) are cited, plus REA Evidence IDs where #18 makes REA usable.

## 7. Done when

1. **Lua tooling:** `luatool` lists every embedded-file registry entry. It extracts, disassembles and decompiles (through the normaliser) every decodable Lua chunk, and reports the success rate per area. Entries the exe can't supply, such as the large scripts cut at 16 KiB in the image, are listed with their reason. The login-stage scripts are readable, whether decompiled or from disassembly. (Amended by D3; owner, 2026-10-12. The spec first said "all 18,587 chunks".)
2. **Login write-up:** `docs/protocol/login.md` documents what gives the client its server address, with evidence.
3. **First connection:** in a sandboxed run, the real client connects to `fenji-stub` (W5).
4. **Census:** `docs/re/m1-script-census.md` gives the server-logic measurement.
5. **Runner:** #17 is done: the tracked runner and watchdog with tests, and GE-Proton pinned.
6. **Archives:** the archive format is parsed as far as it can be without decompression, the Oodle gate is decided, and table dumps follow that decision (or leave M1 by the owner's ruling at the gate).
7. **Records:**
   - spike-0's open M1 questions are answered or carried forward;
   - the parent spec's §10 and §12 get "M1 update" notes;
   - `docs/re/sources.md` lists the external decompiler.

Release: v0.2.0, with the owner's go-ahead.

## 8. Out of scope

- Answering the server hello, and decoding or encoding traffic (M2).
- The launcher, accounts and the ticket (M3).
- A Lua VM (M5).
- Any change to the client.
