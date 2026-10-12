# M1: Data Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. **The owner chose inline execution (superpowers:executing-plans) for M1.**

**Goal:** Make the client's embedded Lua scripts readable (list, extract, disassemble, normalise, decompile). Find what tells the client where its server is, and show it connecting to `fenji-stub`. Measure how much server-side logic the scripts carry. Parse the data archives as far as is possible without Oodle, and put the Oodle choice to the owner.

**Architecture:**
- **`crates/fenji-data`** reads the engine's file *blob* format, which the data archives and the file system embedded in `gujianol.exe` share. It also locates embedded files through the client's own registry tables, and guards every output path into `re/`.
- **`crates/fenji-lua`** is pure bytecode work on the client's Lua 5.3 dialect: parse, write, disassemble, normalise to stock Lua 5.3, and census.
- **`tools/luatool` and `tools/archive`** are thin CLIs over the two crates.
- **Client runs** move to a tracked runner and watchdog in `scripts/spike/`, with GE-Proton pinned.

**Tech Stack:**
- Rust 1.97.1 (edition 2024).
- `lz4_flex` 0.14.0 (LZ4 blocks), `sha2` 0.11.0 (pinned-build check), `thiserror` 2.0.21, `clap` 4.6.
- `mlua` 0.12.2 with `lua53` and `vendored` as a dev-dependency only: real Lua 5.3 compiles our own test sources.
- unluac `2025_12_23` (MIT, Java) as an external decompiler.
- bash, plus Python 3 stdlib for the runner and watchdog.
- GE-Proton11-7, Ghidra 12.1.4 through the `ghidra` MCP.

**Spec:** [`docs/superpowers/specs/2026-10-12-m1-data-layer-design.md`](../specs/2026-10-12-m1-data-layer-design.md), which argues from the parent [`2026-10-11-fenji-design.md`](../specs/2026-10-11-fenji-design.md) and [`docs/re/spike-0.md`](../../re/spike-0.md). Executors read the spec and the plan together.

## Global Constraints

Every task's requirements implicitly include these.

- **Ground rule 3: no game content in git.** That covers extracted files, bytecode, disassembly, decompiled source, stub logs and captures; they live only in the git-ignored `re/`.
  - `scripts/check-forbidden-files.sh` rejects `*.luac` names and any file starting with Lua's bytecode magic `1b 4c 75 61`. So test fixtures are compiled at test time by `mlua` from our own Lua source, and are never committed.
  - Never bypass the check.
- **Tools write only under `re/`** and refuse any other output directory (spec §3).
- **Clean room** (ground rule 2, CLAUDE.md "Clean-room hygiene"):
  - Ghidra work, reading disassembly or decompiled output, and every client run are done only by a session or agent that has never read material rejected in `docs/re/sources.md`. The executing session must confirm that before RE tasks.
  - `docs/` holds prose and tables only: no script text, no decompiled code, no disassembly listings. Addresses, constants, enum names, function names and file names are fine.
  - Tag every finding Observed / Inferred / Unknown, and cite pinned-exe addresses (Ghidra) or REA Evidence IDs.
- **Ground rule 6 gates stop work:** client patching (on disk or in memory), circumventing protection, anything that would load the AGT driver, and Oodle outside its licence. Open a `Decision gate` issue and wait.
- **Ground rule 4:**
  - Client runs happen only via `scripts/spike/isolated-run.sh --no-root --x11 N --cpu-topology`, against `fenji-stub`.
  - `--gf-sdk=mocked` is for sandboxed dev runs only, never for players (gate #11).
  - No client file is ever modified. Inputs the client reads (flags, its own config files) may be added to the run's throwaway copy.
- **Rust conventions:**
  - Rust 1.97.1, edition 2024, `version.workspace = true`, `publish = false`.
  - `unsafe_code = "forbid"`, which applies to tests too.
  - Dependencies are declared once in the root `[workspace.dependencies]`.
  - `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` must pass.
- **Licences:**
  - Linked crates must pass `cargo deny check`: MIT, Apache-2.0, BSD-2/3, ISC, Unicode-3.0 or Zlib.
  - External tools (unluac, GE-Proton) are run, never linked. They are listed in `docs/re/sources.md` before first use.
- **Crate dependency direction** (parent spec §6.1): `fenji-lua` depends on nothing internal; `fenji-data` may depend on `fenji-lua`; tools depend on both.
- **Pinned build:** `bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`, build 10015727 (`docs/re/pinned-build.md`). Tools refuse any other build unless given `--any-build`.
- **Process:**
  - Conventional Commits with scopes `data`, `lua`, `tools`, `re`, `docs` and `deps`.
  - One issue → one branch → one PR per task (`Closes #N`), squash-merged into `dev` once CI is green and the task's checks pass (owner's standing instruction).
  - `main`, tags and releases need the owner's go-ahead.
  - Commit messages end with the `Co-Authored-By` and `Claude-Session` trailers; PR bodies end with the Claude Code line and the session URL.

## Review Focus

These are the five inputs most likely to hurt a user that the spec implies but doesn't spell out. Each has its pinning test in the task that owns the code.

1. **Undecodable embedded entries.** 79 large files are cut at 16 KiB in the exe (see "Planning findings"; withdrawn by E2: none is, so this item now means malformed headers, blocks and pieces). They must yield a per-entry error that is counted and reported. They must never cause a panic, abort a batch, or read outside the image. Tests: Task 3 `truncated_entry_is_an_error_not_a_panic` and `read_outside_image_is_none`; Task 6 summary counts.
2. **Malformed bytecode:** truncation at any byte, absurd counts, unknown constant tags, extreme nesting and trailing bytes. Each gives a typed error, never a panic or a huge allocation. Tests: Task 4 `every_truncation_is_an_error`, `absurd_count_is_refused`, `deep_nesting_is_refused`, `unknown_constant_tag_is_refused` and `trailing_bytes_are_refused`.
3. **Output location.** Each of these is refused, and nothing is written outside `re/`:
   - `--out` outside `re/`, through `..` or through a symlink that leaves `re/`;
   - registry paths with `..`, an absolute path, a backslash or a drive letter.

   Tests: Task 3 `out_dir_*` and `safe_relative_*`; Task 6 `cli_refuses_out_dir_outside_re`.
4. **The wrong client build.** A different `gujianol.exe` must be refused with a clear message naming both hashes, never silently mis-parsed. Tests: Task 6 `cli_refuses_unpinned_build`.
5. **Watchdog false positives and negatives:**
   - Must not trigger: Proton re-creating its built-in driver links (the owner's run look-1), and Wine's split `winebth` log line (run 7).
   - Must trigger: a new or genuinely changed driver file, a native `.sys` load, a `"PE"` resource lookup, a non-baseline `LoadDriver` or `CreateService`, and the AGT strings.

   Tests: Task 9 `test-watchdog.py`.

## Planning findings (read-only probes, 2026-10-12)

These are facts the tasks build on. They are local evidence from planning probes, with scripts copied to `re/m1/probes/` in Task 1. Task 1 re-verifies them and writes them up properly. Task 1 corrected the registry layout, the counts, the truncation finding and the `TRYSELF` mapping: see "Execution amendments" E1–E3.

- **The registry (Observed):** the embedded files are listed by 76 runs of 24-byte triples in `.data`. Each triple is (pointer to a NUL-terminated path in `.rdata`, `u64` stored size, pointer to a blob header in `.rdata`).
  - Example: the triple at VA `0x143181fc8` maps `combat/skill/3007163.luac` to the header at `0x1413e9d40`.
  - The runs hold 21,425 names: 20,360 `.luac`, 638 `.xml`, 411 `.toc`, 7 `.config`, 6 `.c`, 2 `.h` and 1 `.txt`. Together they point at 19,566 distinct headers.
  - Top-level directories include `combat` (18,720 names), `addons` (1,767), `modules` (534) and `common`.
  - There is a login addon (`addons/Login/Login.luac` and its `.xml`, `.toc` and `_i18n.xml`). Its manifest exports `GetGFChooseServerFrame`.
  - Per-distribution `load.config` files exist for `gameforge`, `steam`, `kr`, `vi`, `mysg` and `common`. They select a locale overlay and list table overrides.
- **The blob header (Observed)**, little-endian:
  - `u64 raw_size`, `u64 timestamp` (Unix seconds, 2022-11-25 for this build), `u64 stored_size`, `u32 block_size`, `u16 codec`, `u16 check`;
  - then one `u32` compressed size per block, then the blocks.
  - `stored_size` = 32 + 4 × blocks + the sum of the block sizes.
  - Every embedded entry is codec 1 (LZ4) with one block. `_index` uses the same header with codec 4 (Oodle), a 0x80000 block size and 49 blocks.
  - The meaning of `check` is Unknown.
- **LZ4 (Observed):** 18,488 of the 18,587 Lua chunks decode as a single LZ4 block to exactly `raw_size` bytes.
- **Truncation (Observed):** every entry whose stored size exceeds 16,384 is cut at 16,384 bytes in the image. In 66 of the 79 large Lua entries, another object starts exactly at header + 16,384, and none of them decodes.
- **The Lua dialect (Observed):**
  - The header omits the `size_t` size byte (`04 04 08 08`), but the dump still writes long string lengths as 8-byte `size_t`.
  - The rest is the stock Lua 5.3 dump layout: every decodable chunk parses to its exact end.
  - All 89,127 functions end in opcode 38 (stock `RETURN`).
  - All chunks are stripped: no source names, line info, local names or upvalue names.
- **The extra opcodes (Observed):** of 2,825,378 instructions, only 49 `TRYTABUP` (118,843), 50 `TRYLIBUP` (38,349) and 53 `TRYSELF` (262,743) occur. In every occurrence B is a register or upvalue index and C an RK constant.
- **unluac (Observed):**
  - `unluac_2025_12_23.jar` decompiles stripped stock Lua 5.3, our own test source compiled with `luac -s`.
  - On client chunks with only the header fixed, it stops at the first extra opcode.
  - With a naive mapping (49 and 50 → `GETTABUP`, 53 → `SELF`) it decompiles three sample chunks to readable source.

## Deviations from the spec (for the owner's review of this plan)

- **D1. Where locating and decompressing live.** The spec put `locate` and `lz4` in `fenji-lua`. The embedded scripts turned out to be files in the engine's blob format, shared with the archives, and `fenji-lua` may not depend on `fenji-data`. So the blob reader, the codecs and the embedded-file-system reader live in `fenji-data` (`blob`, `codec`, `exe_fs`), and `fenji-lua` stays pure bytecode.
- **D2. How chunks are located.** `locate` uses the client's registry, found in planning, not the `F6 04` scan fallback.
- **D3. Done-when 1's chunk count.** Spec Done-when 1 says all 18,587 chunks. 79 large scripts are truncated in the exe, so they can't be extracted from it; complete copies may be in the archives, behind the Oodle gate. Done-when 1 becomes:
  - every registry entry is listed;
  - every decodable one is extracted and disassembled;
  - undecodable entries are listed with their reason.

  Task 2's gate write-up names this as one reason the archives matter.
- **D4. New dependencies:** `sha2` (the pinned-build check) and `mlua` (test-only, a real Lua 5.3 for fixtures). Both are MIT or MIT/Apache.

**The owner's decisions (2026-10-12):** D1–D4 accepted, and the fresh reviews before merging Tasks 3 and 9 kept. Plan approved, executed inline. The M1 spec's Status, §3 Units and Done-when 1 carry the amendments.

## Execution amendments (from Task 1, 2026-10-12)

Task 1 ([`m1-script-format.md`](../../re/m1-script-format.md)) corrected three planning findings. Each is a ruling in the execution ledger. Where a task's text below still says otherwise, these amendments apply.

- **E1. The registry is a mount table, and large files span several entries.** A mount table at `0x143197720` lists five directories: `data/c`, `data/locale`, `data/core`, `data/interface` and `data/scripts`. Each is a NULL-terminated list of 24-byte entries: path pointer, `u32` stored size, `u32` continuation flag, piece pointer. A file whose flag is set continues in the following entries, and its pieces (0x3FFC bytes each, except the last) are concatenated. Walked that way there are 21,512 files (20,443 `.luac`). All of them decode, and all the chunks parse (100,465 functions). This changes two tasks:
  - Task 3: `exe_fs` walks the mount table and joins multi-piece files instead of scanning `.data` for triples. `ExeEntry` carries its directory and piece addresses rather than one header address. `test_util` builds a mount table that includes a multi-piece file, and Task 3 gains tests for multi-piece decoding.
  - Task 6: the pinned test expects 21,512 entries and none undecodable.
- **E2. No embedded file is truncated.** The "79 large scripts cut at 16 KiB" were the first pieces of multi-piece files. Review Focus 1 still applies to malformed input (a bad header, a bad block, a piece outside the image): each gives a per-entry error, never a panic. Tasks 2, 12 and 13 drop "complete copies of truncated scripts" from what Oodle would unlock. D3's wording stands; for the pinned build its list of undecodable entries is empty.
- **E3. `TRYSELF` is removed, not mapped to `SELF`.** Every `TRYSELF` is a caching prefix right before an identical `SELF`. Its stock equivalent is therefore nothing, which is what the spec's "where their meaning allows" admits. Normalisation removes it and re-targets jumps (m1-script-format §4). `TRYTABUP` and `TRYLIBUP` still become `GETTABUP`, and `NOP`, `XGETTABUP`, `XGETSELF` and `XSELF` stay unmapped (they never occur in the dumps). Task 7's normaliser takes one of four actions per opcode (keep, replace, remove, refuse), re-targets jumps, and gains tests for removal and re-targeting.

## File map

| Path | Task | Responsibility |
|---|---|---|
| `docs/re/m1-script-format.md` | 1 | RE write-up: embedded file system, blob header, Lua dialect, extra-opcode semantics, undecodable entries |
| `crates/fenji-data/src/{lib,error,pe,blob,codec,exe_fs,re_dir,test_util}.rs` | 3 | PE sections, blob header and decoding, codecs (stored, LZ4; Oodle refused), embedded file system via the registry, `re/` output guard, synthetic test images |
| `crates/fenji-data/src/archive.rs` | 12 | Archive part headers and record tables, `_index` header |
| `crates/fenji-lua/src/{lib,error,opcode,chunk}.rs` | 4 | Opcodes (stock + client), chunk model, parse and write in both dialects |
| `crates/fenji-lua/src/disasm.rs` | 5 | Disassembler |
| `crates/fenji-lua/src/normalize.rs` | 7 | Client → stock Lua 5.3 |
| `crates/fenji-lua/src/census.rs` | 11 | Server-logic measurement |
| `crates/fenji-lua/tests/{common/mod.rs,chunk.rs,disasm.rs,normalize.rs,census.rs}` | 4, 5, 7, 11 | Tests against real Lua 5.3 (`mlua`) |
| `tools/luatool/` (`fenji-luatool`, binary `luatool`) | 6, 7, 11 | CLI: `list`, `extract`, `disasm`, `normalize`, `decompile`, `census` |
| `tools/archive/` (`fenji-archive`, binary `archive`) | 12 | CLI: `info`, `list`, `extract --stored-only`, `index` |
| `scripts/spike/run-client.sh`, `scripts/spike/watchdog.py`, `scripts/spike/test-watchdog.py` | 9 | Tracked client runner and AGT watchdog, with tests |
| `scripts/versions.env`, `scripts/bootstrap.sh`, `CLAUDE.md`, `docs/development.md` | 7, 9 | unluac and GE-Proton pins |
| `docs/re/sources.md` | 4, 7, 9 | Lua 5.3 via mlua, unluac, GE-Proton |
| `docs/protocol/login.md` | 8, 10 | What gives the client its server address; first-connection evidence |
| `docs/re/m1-script-census.md` | 11 | Server-logic measurement |
| `docs/re/m1-archives.md` | 12 | Archive structure without decompression |
| `docs/re/m1-summary.md`, spec notes | 14 | Spike-0's M1 questions answered; "M1 update" notes |
| `.github/workflows/ci.yml` | 9 | Watchdog tests in the `repo-hygiene` job |

## Order

The critical path is Task 1 → 3 → 4 → 5 → 6 → 7 → 8 → 10.
- Task 2 goes right after Task 1, so the owner can decide the Oodle gate while the Lua work proceeds.
- Task 9 must precede Task 10.
- Tasks 11 and 12 follow.
- Task 13 starts when the owner decides the gate.
- Task 14 closes M1.

---

### Task 0: M1 tracking

**Files:** none (GitHub only).

- [ ] **Step 1: Create one `Task` issue per task (1–14)** in milestone `M1 Data layer`, using `.github/ISSUE_TEMPLATE/task.yml`'s fields (Goal, Plan reference, Done when):

```bash
for t in "1|Script format RE write-up (registry, blob, Lua dialect, extra opcodes)|type:re,area:docs" \
         "2|Oodle decision gate: options for reading the archives|type:re,area:docs" \
         "3|fenji-data: PE, blob, codecs, embedded file system, re/ guard|type:task,area:data" \
         "4|fenji-lua: opcodes, chunk model, parse/write in both dialects|type:task,area:lua" \
         "5|fenji-lua: disassembler|type:task,area:lua" \
         "6|luatool: list, extract, disasm|type:task,area:tools" \
         "7|Normaliser, luatool normalize/decompile, unluac pin|type:task,area:lua" \
         "8|Login-stage hunt: what gives the client its server address|type:re,area:docs" \
         "9|Tracked client runner, AGT watchdog, pinned GE-Proton|type:task,area:dev-env" \
         "10|First connection: the real client reaches fenji-stub|type:re,area:docs" \
         "11|Script census: server-side logic in the embedded scripts|type:task,area:lua" \
         "12|Archive structure without decompression; archive tool|type:task,area:data" \
         "13|Oodle decision follow-through|type:task,area:data" \
         "14|M1 close-out: records, spec notes, final review, v0.2.0|type:task,area:docs"; do
  IFS='|' read -r n title labels <<<"$t"
  gh issue create --milestone "M1 Data layer" --title "$title" \
    $(printf -- '--label %s ' ${labels//,/ }) \
    --body "### Goal
${title}.

### Plan reference
docs/superpowers/plans/2026-10-12-m1-data-layer.md, Task ${n}

### Done when
The task's steps in the plan are complete and its PR is merged into dev."
done
```

Expected: 14 issue URLs. Task 9 duplicates #17, so close #17 as a duplicate of Task 9's issue: `gh issue close 17 --comment "Tracked as #<task-9 issue> (M1 plan Task 9)"`.

- [ ] **Step 2: Record the issue numbers** in a table at the top of the local ledger `re/m1/progress.md` (git-ignored). Use `Task N → #M`, and a line per task as it completes, in the M0 ledger style. Branch names below use `<issue>` for the task's own number.

---

### Task 1: Script format RE write-up

**Files:**
- Create: `docs/re/m1-script-format.md`
- Modify: `docs/README.md` (one row)
- Local only: `re/m1/probes/`, `re/m1/evidence/`

**Interfaces:**
- Produces, for Tasks 3–7:
  - the registry triple layout;
  - the blob header layout, and the codec numbers 0 stored, 1 LZ4, 4 Oodle;
  - the client header bytes (`04 04 08 08`, no `size_t` byte);
  - the opcode numbering: 0–46 stock, 47–53 = `NOP`, `XGETTABUP`, `TRYTABUP`, `TRYLIBUP`, `XGETSELF`, `XSELF`, `TRYSELF`;
  - the stock equivalent of each client-only opcode that `normalize::stock_equivalent` uses (Task 7).
- Task 7's mapping must equal this doc's §4 table.

- [ ] **Step 1: Branch, and keep the planning probes as evidence.** Confirm this session has never read material rejected in `docs/re/sources.md`; if it has, stop and hand this task to a clean agent.

```bash
git switch dev && git pull --ff-only && git switch -c docs/re-<issue>-script-format
mkdir -p re/m1/probes re/m1/evidence
cp /tmp/claude-1000/-home-jdarkovski-projects-fenji/*/scratchpad/m1/probe_*.py re/m1/probes/ 2>/dev/null || true
```

If the scratchpad copies are gone, rewrite the two probes that matter as stdlib-only scripts in `re/m1/probes/`:
- `registry.py`: walk `.data` 8-byte-aligned, accept triples whose two pointers land in `.rdata` and whose header at +16 equals the stored size, and print counts by extension and top directory.
- `lua53.py`: LZ4-decode each registry entry, then parse with the stock 5.3 layout after a 32-byte client header.

Run each with `python3 -I re/m1/probes/<x>.py ~/fenji-client/bin64/gujianol.exe > re/m1/evidence/<x>.txt`.

- [ ] **Step 2: Re-verify the planning numbers from the registry**, not from the `ESC Lua` scan:
  - names and distinct headers;
  - names per extension;
  - entries by codec;
  - Lua entries that decode, and that parse in the client dialect;
  - entries that don't decode, listed with stored size and path into `re/m1/evidence/undecodable.tsv`;
  - names that appear more than once. A big file split across several registry entries would show up here.

Expected:
- 21,425 names and 19,566 distinct headers;
- every entry is codec 1;
- every undecodable entry has a stored size above 16,384.

If any number differs, the registry probe is the authority. Record the real numbers; Task 6's integration test uses them.

- [ ] **Step 3: Account for the 24 `ESC Lua` hits that aren't registry entries,** found by the magic scan (18,587 hits against the registry's Lua entries). For each, record the offset, and whether it falls inside another entry's data (a chunk inside a larger file) or inside a truncated entry's tail. Write the result to `re/m1/evidence/stray-magic.tsv`.

- [ ] **Step 4: Ghidra: the registry walker.** Read-only, through the `ghidra` MCP. No renames, comments, scripts or `debugger_*` tools.
  - `get_xrefs_to` on the first VA of a few registry runs. The first run found in planning is at VA `0x14314a810`; the others come from Step 2's probe.
  - `force_decompile` the function or functions that read them: they register embedded files with the engine's virtual file system.
  - Record:
    - how a lookup goes from path to entry;
    - whether the engine reads `stored_size` bytes from the header pointer (so the 79 large entries are truncated for the engine too);
    - whether there is a fallback, such as archives first and embedded copies second;
    - the precedence between mounted archives (`--fs=`) and embedded files.

  Tag code readings Inferred.

- [ ] **Step 5: Ghidra: the VM's semantics for the extra opcodes.**
  - Find `luaV_execute`: the function that masks the instruction with `0x3F` and dispatches through a jump table of at least 54 entries. Try `search_instructions` for `and` with `0x3f` near an indirect `jmp`, or follow the callers of the `OP_RETURN` epilogue.
  - Confirm it by reading cases 0 (`MOVE`), 1 (`LOADK`) and 38 (`RETURN`).
  - Then read cases 47–53. For each, record its operand use (A, B, C or Bx; RK or not), what it reads and writes, and how it differs from its stock namesake. For example, does `TRYTABUP` return nil instead of raising when the key or the table is missing? Does `TRYSELF` skip `R(A+1) := R(B)`?
  - Decide each opcode's stock equivalent for normalisation. The constraint is one instruction for one instruction, with the same operands, and the closest stock semantics for reading. If none is close enough, say so: the opcode stays unmapped and its chunks are refused. The planning default, to confirm or replace: `TRYTABUP` and `TRYLIBUP` → `GETTABUP`, `TRYSELF` → `SELF`, the other four unmapped (they don't occur).

- [ ] **Step 6: Write `docs/re/m1-script-format.md`.** Prose and tables only; findings tagged Observed / Inferred / Unknown with evidence keys. Sections:
  - `# M1: the client's embedded scripts`, with a two-sentence summary and a Method line that names the pinned exe SHA-256 and says nothing was executed.
  - `## 1. The embedded file system`: the registry tables (VA ranges, triple layout), counts by extension and top directory, the login addon and the `load.config` files (names only), and Step 4's lookup reading.
  - `## 2. The blob header`: a field table with offsets, codecs, the stored-size identity and the `_index` comparison. `check` is Unknown.
  - `## 3. The Lua dialect`: the header bytes, the 8-byte `size_t` string lengths, stripping, and the opcode numbering evidence (89,127 functions end in 38).
  - `## 4. Extra opcodes`: usage counts, the VM reading per opcode (VM case addresses) and **the normalisation table** (client opcode → stock opcode, or "unmapped").
  - `## 5. Entries the exe can't supply`: the truncated entries (count, the 16,384 threshold, the evidence), the stray magic hits, and what Step 4 says the engine does about them.
  - `## Evidence index`: key → local file under `re/m1/` or a Ghidra address.

- [ ] **Step 7: Add the docs row and commit.** Add to `docs/README.md`:

```markdown
| [`re/m1-script-format.md`](re/m1-script-format.md) | M1: the client's embedded file system, blob format, Lua 5.3 dialect and extra opcodes. |
```

```bash
scripts/check-forbidden-files.sh
git add docs/re/m1-script-format.md docs/README.md
git commit -m "docs(re): write up the client's embedded script format" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

Wait for CI (`gh pr checks <n> --watch`), then squash-merge with `gh pr merge <n> --squash`.

---

### Task 2: Oodle decision gate

**Files:** none in git (the GitHub issue only). Local notes go in `re/m1/oodle-options.md`.

**Interfaces:**
- Consumes Task 1 §2 (the codec numbers) and §5 (the truncated scripts).
- Produces a `Decision gate` issue. Tasks 12–13's decompression work waits on it.

- [ ] **Step 1: Research option (b), licensed external decoders.** Read only licences, READMEs and provenance statements; download and run nothing. For each candidate, record:
  - name and URL;
  - licence (SPDX), and whether it is clearly stated in the repository;
  - provenance: clean-room, or derived from RE of Oodle;
  - which Oodle codecs it decodes (Kraken required);
  - maintenance;
  - whether it needs an Oodle DLL.

  Search with WebSearch standard mode: "Oodle Kraken decompressor open source", "ooz Kraken decoder licence", "oodle decompress rust crate". Reject any candidate whose licence isn't clearly stated.

- [ ] **Step 2: Assess options (a), (c) and (d).**
  - **(a) Clean-room Rust decoder.** Size the Kraken format work (decoder size in comparable projects), and say where the format knowledge would come from. Reading an RE-derived decoder's source to write ours makes ours derived from it, which is a provenance problem.
  - **(c) The client's own `oo2core_6_win64.dll`** (spike-0 1.x), in a sandboxed Wine helper, on the owner's machine only. Using the middleware outside the game is the explicit rule-6 gate. State the licence question plainly.
  - **(d) Defer tables beyond M1.** Note the cost: the 79 truncated scripts' complete copies, if the archives hold them, stay unreadable too.

- [ ] **Step 3: Open the gate issue** with the decision-gate template's sections. The body:

```markdown
### What we found
- The data archives (`data000`–`data099`) and `_index` store files in the engine's blob format, with codec 4 for most entries (spike-0 6.5–6.9; `docs/re/m1-script-format.md` §2). Codec 4 blocks begin `8C 06`, an Oodle LZ (Kraken) block header (Inferred).
- `_index` is one codec-4 blob of 49 × 512 KiB blocks. Without decoding it, no archive path names are readable, and so no tables (items, skills, monsters, XP curve, maps).
- 79 large embedded scripts are truncated at 16 KiB in `gujianol.exe` (m1-script-format §5). If the archives hold complete copies, they are behind codec 4 too.

### Options
| Option | What it means | Legal / provenance risk | Effort |
|---|---|---|---|
| (a) Clean-room Kraken decoder in Rust | <from Step 2> | <…> | <…> |
| (b) <named external decoder>, external tool only, output in `re/` | <from Step 1> | <…> | <…> |
| (c) The client's own `oo2core_6_win64.dll` in a sandboxed helper | <from Step 2> | middleware outside the game: rule-6 gate | <…> |
| (d) Defer tables beyond M1 | M1 closes on everything else; table dumps move to a follow-up | none new | none now |

**Recommendation (controller, not legal advice):** <one option, with the reasoning in 2–3 sentences>.

### Decision
_To be filled in by the owner. Archive decompression and table dumps (plan Tasks 12–13) wait for this._
```

Fill every `<…>` cell from Steps 1–2 before opening. Create it with:

```bash
gh issue create --title "Decision gate: decompressing the archives (Oodle)" \
  --label decision-gate --label status:blocked --label type:re --milestone "M1 Data layer" \
  --body-file re/m1/oodle-gate.md
```

- [ ] **Step 4: Tell the owner** in chat, with the issue link and the recommendation in one line. Don't wait: continue with Task 3.

---

### Task 3: `fenji-data`: PE sections, blob format, codecs, embedded file system, `re/` guard

**Files:**
- Modify: `Cargo.toml` (workspace members and dependencies), `Cargo.lock`
- Create:
  - `crates/fenji-data/Cargo.toml`
  - `crates/fenji-data/src/lib.rs`
  - `crates/fenji-data/src/error.rs`
  - `crates/fenji-data/src/pe.rs`
  - `crates/fenji-data/src/blob.rs`
  - `crates/fenji-data/src/codec.rs`
  - `crates/fenji-data/src/exe_fs.rs`
  - `crates/fenji-data/src/re_dir.rs`
  - `crates/fenji-data/src/test_util.rs`
- Tests: a unit-test module in each source file.

**Interfaces:**
- Consumes Task 1 §1–§2: the registry triple and blob header layouts.
- Produces, for Tasks 6, 7, 11 and 12:
  - `fenji_data::PeImage::parse(Vec<u8>) -> Result<PeImage>`, with `.read(va: u64, len: usize) -> Option<&[u8]>`, `.c_str(va, max) -> Option<&[u8]>`, `.section(name) -> Option<&Section>`, `.section_of(va) -> Option<&Section>`, `.section_data(&Section) -> &[u8]`, `.image_base() -> u64`
  - `fenji_data::BlobHeader::parse(&[u8], at: u64) -> Result<BlobHeader>`, with fields `raw_size`, `timestamp` and `stored_size` (u64), `block_size: u32`, `codec: Codec`, `check: u16` and `block_sizes: Vec<u32>`; plus `.header_len()` and `.data_len()`
  - `fenji_data::Codec::{Stored, Lz4, Oodle, Other(u16)}`, with `from_u16`, `as_u16` and `name`
  - `fenji_data::decode_blob(&[u8], at: u64, &dyn Decompressor) -> Result<Vec<u8>>`
  - `fenji_data::Decompressor` (trait: `decompress_block(&self, Codec, input: &[u8], raw_len: usize, index: usize) -> Result<Vec<u8>>`) and `fenji_data::BuiltinCodecs`
  - `fenji_data::ExeFs::scan(&PeImage) -> Result<ExeFs>`, with `.entries() -> &[ExeEntry]`, `.header(&ExeEntry) -> Result<BlobHeader>` and `.read(&ExeEntry, &dyn Decompressor) -> Result<Vec<u8>>`
  - `ExeEntry { path: String, stored_size: u64, header_va: u64 }`
  - `fenji_data::re_dir::{find_repo_root(&Path) -> Result<PathBuf>, resolve_out_dir(repo: &Path, requested: &Path) -> Result<PathBuf>, safe_relative(&str) -> Option<PathBuf>}`
  - `fenji_data::PINNED_EXE_SHA256: &str` and `fenji_data::sha256_hex(&[u8]) -> String`
  - Behind feature `test-util`: `fenji_data::test_util::{pe_image, lz4_blob, exe_with_files, IMAGE_BASE}`

- [ ] **Step 1: Branch and workspace entries.**

```bash
git switch dev && git pull --ff-only && git switch -c feat/data-<issue>-exe-fs
```

In the root `Cargo.toml`, set `members = ["tools/stub", "crates/fenji-data"]` and add to `[workspace.dependencies]`, keeping the list alphabetical:

```toml
fenji-data = { path = "crates/fenji-data" }
lz4_flex = { version = "0.14.0", default-features = false, features = ["std", "safe-encode", "safe-decode"] }
sha2 = "0.11.0"
thiserror = "2.0.21"
```

Create `crates/fenji-data/Cargo.toml`:

```toml
[package]
name = "fenji-data"
description = "Reads the SoLO client's own data: the engine's blob format, the file system embedded in gujianol.exe and the archives' structure"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[features]
# Synthetic PE images and blobs for tests in this and other crates: never game bytes.
test-util = []

[dependencies]
lz4_flex.workspace = true
sha2.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write `src/lib.rs`, `src/error.rs` and `src/test_util.rs`.** The test helpers come first, because every test below builds its input with them.

`crates/fenji-data/src/lib.rs`:

```rust
//! Reads the SoLO client's own data at run time (design spec §6.3; M1 spec §2–§5). Nothing it
//! reads is ever committed: tools write only under `re/` ([`re_dir`]).
mod blob;
mod codec;
mod error;
mod exe_fs;
mod pe;
pub mod re_dir;
#[cfg(any(test, feature = "test-util"))]
pub mod test_util;

pub use blob::{decode as decode_blob, BlobHeader, Codec, FIXED_LEN};
pub use codec::{BuiltinCodecs, Decompressor};
pub use error::{Error, Result};
pub use exe_fs::{ExeEntry, ExeFs};
pub use pe::{PeImage, Section};

/// SHA-256 of the pinned `bin64/gujianol.exe` (docs/re/pinned-build.md).
pub const PINNED_EXE_SHA256: &str =
    "3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678";

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha256_hex_matches_the_standard_vector() {
        assert_eq!(
            super::sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
```

`crates/fenji-data/src/error.rs`:

```rust
/// Everything that can go wrong while reading the client's data.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a PE32+ image: {0}")]
    NotPe(&'static str),
    #[error("blob header at {at:#x}: {reason}")]
    BadBlob { at: u64, reason: String },
    #[error("codec {0} is not available here (Oodle waits on the M1 decision gate)")]
    CodecUnavailable(u16),
    #[error("LZ4 block {index}: {reason}")]
    Lz4 { index: usize, reason: String },
    #[error("decoded {got} bytes where the header says {want}")]
    SizeMismatch { got: u64, want: u64 },
    #[error("{len} bytes at {va:#x} are not all in one section's file data")]
    OutOfImage { va: u64, len: u64 },
    #[error("no embedded-file registry found in .data")]
    NoRegistry,
    #[error("output directory: {0}")]
    OutDir(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
```

`crates/fenji-data/src/test_util.rs`:

```rust
//! Synthetic inputs for tests, built from our own bytes: small PE32+ images, LZ4 blobs, and an
//! embedded file system with its registry. Never game bytes.
use crate::blob::FIXED_LEN;

/// The image base of every synthetic image (the client's own is the same).
pub const IMAGE_BASE: u64 = 0x1_4000_0000;

/// A PE32+ image whose sections are (name, RVA, file bytes), in order.
pub fn pe_image(sections: &[(&str, u32, Vec<u8>)]) -> Vec<u8> {
    const OPT_SIZE: usize = 0xF0;
    let headers = 0x40 + 4 + 20 + OPT_SIZE + 40 * sections.len();
    let mut raw_offset = headers.next_multiple_of(0x200);
    let mut out = vec![0u8; raw_offset];
    out[0..2].copy_from_slice(b"MZ");
    out[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    out[0x40..0x44].copy_from_slice(b"PE\0\0");
    let coff = 0x44;
    out[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
    out[coff + 2..coff + 4].copy_from_slice(&(sections.len() as u16).to_le_bytes());
    out[coff + 16..coff + 18].copy_from_slice(&(OPT_SIZE as u16).to_le_bytes());
    let opt = coff + 20;
    out[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
    out[opt + 24..opt + 32].copy_from_slice(&IMAGE_BASE.to_le_bytes());
    let mut entry = opt + OPT_SIZE;
    let mut data = Vec::new();
    for (name, rva, bytes) in sections {
        let mut n = [0u8; 8];
        n[..name.len()].copy_from_slice(name.as_bytes());
        out[entry..entry + 8].copy_from_slice(&n);
        out[entry + 8..entry + 12].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        out[entry + 12..entry + 16].copy_from_slice(&rva.to_le_bytes());
        out[entry + 16..entry + 20].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        out[entry + 20..entry + 24].copy_from_slice(&(raw_offset as u32).to_le_bytes());
        entry += 40;
        let padded = bytes.len().next_multiple_of(0x200);
        data.extend_from_slice(bytes);
        data.resize(data.len() + (padded - bytes.len()), 0);
        raw_offset += padded;
    }
    out.extend_from_slice(&data);
    out
}

/// A one-block LZ4 blob, the way the engine stores a small file.
pub fn lz4_blob(content: &[u8], timestamp: u64) -> Vec<u8> {
    let comp = lz4_flex::block::compress(content);
    let mut b = Vec::with_capacity(FIXED_LEN + 4 + comp.len());
    b.extend_from_slice(&(content.len() as u64).to_le_bytes());
    b.extend_from_slice(&timestamp.to_le_bytes());
    b.extend_from_slice(&((FIXED_LEN + 4 + comp.len()) as u64).to_le_bytes());
    b.extend_from_slice(&(content.len() as u32).to_le_bytes()); // block size: the whole file
    b.extend_from_slice(&1u16.to_le_bytes()); // codec 1: LZ4
    b.extend_from_slice(&0u16.to_le_bytes()); // check: meaning unknown
    b.extend_from_slice(&(comp.len() as u32).to_le_bytes());
    b.extend_from_slice(&comp);
    b
}

/// An image with an embedded file system: `.rdata` holds each path and its blob, `.data` noise
/// and then the registry triples. Returns the image and each file's header VA.
pub fn exe_with_files(files: &[(&str, Vec<u8>)]) -> (Vec<u8>, Vec<u64>) {
    const RDATA: u32 = 0x1000;
    const DATA: u32 = 0x10_0000;
    let (mut rdata, mut registry, mut vas) = (Vec::new(), Vec::new(), Vec::new());
    for (path, blob) in files {
        let path_va = IMAGE_BASE + u64::from(RDATA) + rdata.len() as u64;
        rdata.extend_from_slice(path.as_bytes());
        rdata.push(0);
        rdata.resize(rdata.len().next_multiple_of(8), 0);
        let header_va = IMAGE_BASE + u64::from(RDATA) + rdata.len() as u64;
        rdata.extend_from_slice(blob);
        rdata.resize(rdata.len().next_multiple_of(8), 0);
        registry.extend_from_slice(&path_va.to_le_bytes());
        registry.extend_from_slice(&(blob.len() as u64).to_le_bytes());
        registry.extend_from_slice(&header_va.to_le_bytes());
        vas.push(header_va);
    }
    let mut data = vec![0xAB; 40]; // not a registry
    data.extend_from_slice(&registry);
    data.extend_from_slice(&[0; 24]);
    let image = pe_image(&[(".text", 0x800, vec![0xC3; 16]), (".rdata", RDATA, rdata), (".data", DATA, data)]);
    (image, vas)
}
```

- [ ] **Step 3: Write the failing tests for `pe`, `blob`, `exe_fs` and `re_dir`.** Each block below is the `#[cfg(test)] mod tests` at the bottom of its file. Create each file with only `use crate::{Error, Result};` above its test module for now.

`src/pe.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{pe_image, IMAGE_BASE};

    #[test]
    fn maps_virtual_addresses_to_file_bytes() {
        let img = PeImage::parse(pe_image(&[
            (".rdata", 0x1000, b"abc\0def".to_vec()),
            (".data", 0x2000, vec![9; 16]),
        ]))
        .unwrap();
        assert_eq!(img.image_base(), IMAGE_BASE);
        assert_eq!(img.read(IMAGE_BASE + 0x1004, 3), Some(b"def".as_slice()));
        assert_eq!(img.c_str(IMAGE_BASE + 0x1000, 16), Some(b"abc".as_slice()));
        assert_eq!(img.section_of(IMAGE_BASE + 0x2000).map(|s| s.name.as_str()), Some(".data"));
    }

    #[test]
    fn read_outside_image_is_none() {
        let img = PeImage::parse(pe_image(&[(".rdata", 0x1000, vec![1; 8])])).unwrap();
        assert_eq!(img.read(IMAGE_BASE + 0x1006, 4), None); // runs past the section's file data
        assert_eq!(img.read(IMAGE_BASE + 0x5000, 1), None); // in no section
        assert_eq!(img.read(u64::MAX - 1, 4), None);
        assert_eq!(img.c_str(IMAGE_BASE + 0x1000, 16), None); // no NUL before the end
    }

    #[test]
    fn rejects_what_is_not_pe32_plus() {
        assert!(matches!(PeImage::parse(b"not a PE".to_vec()), Err(Error::NotPe(_))));
        let mut pe = pe_image(&[(".rdata", 0x1000, vec![0; 4])]);
        pe[0x58..0x5A].copy_from_slice(&0x10Bu16.to_le_bytes()); // PE32, not PE32+
        assert!(matches!(PeImage::parse(pe), Err(Error::NotPe("not PE32+"))));
    }
}
```

`src/blob.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_util::lz4_blob, BuiltinCodecs};

    #[test]
    fn single_block_lz4_round_trip() {
        let content = b"local x = 1 ".repeat(40);
        let blob = lz4_blob(&content, 1_669_366_433);
        let h = BlobHeader::parse(&blob, 0).unwrap();
        assert_eq!((h.raw_size, h.timestamp, h.codec), (content.len() as u64, 1_669_366_433, Codec::Lz4));
        assert_eq!(h.block_sizes.len(), 1);
        assert_eq!(decode(&blob, 0, &BuiltinCodecs).unwrap(), content);
    }

    #[test]
    fn multi_block_lz4() {
        let content: Vec<u8> = (0..10_000u32).flat_map(|i| (i % 251).to_le_bytes()).collect();
        let block = 16_384usize;
        let blocks: Vec<Vec<u8>> = content.chunks(block).map(lz4_flex::block::compress).collect();
        let stored = FIXED_LEN + 4 * blocks.len() + blocks.iter().map(Vec::len).sum::<usize>();
        let mut b = Vec::new();
        b.extend_from_slice(&(content.len() as u64).to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&(stored as u64).to_le_bytes());
        b.extend_from_slice(&(block as u32).to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        for c in &blocks {
            b.extend_from_slice(&(c.len() as u32).to_le_bytes());
        }
        for c in &blocks {
            b.extend_from_slice(c);
        }
        assert_eq!(BlobHeader::parse(&b, 0).unwrap().block_sizes.len(), 3);
        assert_eq!(decode(&b, 0, &BuiltinCodecs).unwrap(), content);
    }

    #[test]
    fn inconsistent_stored_size_is_refused() {
        let mut blob = lz4_blob(b"abcabcabc", 0);
        blob[16] ^= 1;
        assert!(matches!(BlobHeader::parse(&blob, 0x10), Err(Error::BadBlob { at: 0x10, .. })));
    }

    #[test]
    fn oodle_is_refused_until_the_gate() {
        let mut blob = lz4_blob(b"abcabcabc", 0);
        blob[28] = 4;
        assert!(matches!(decode(&blob, 0, &BuiltinCodecs), Err(Error::CodecUnavailable(4))));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let blob = lz4_blob(b"abcdefabcdef", 0);
        for n in 0..blob.len() {
            assert!(decode(&blob[..n], 0, &BuiltinCodecs).is_err(), "prefix of {n} bytes");
        }
    }
}
```

`src/exe_fs.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{exe_with_files, lz4_blob, pe_image};
    use crate::BuiltinCodecs;

    #[test]
    fn scan_finds_registered_files_and_reads_them() {
        let (pe, vas) = exe_with_files(&[
            ("a/x.luac", lz4_blob(&b"hello ".repeat(30), 7)),
            ("ui/y.xml", lz4_blob(b"<UI/>", 7)),
        ]);
        let img = PeImage::parse(pe).unwrap();
        let fs = ExeFs::scan(&img).unwrap();
        let got: Vec<(&str, u64)> = fs.entries().iter().map(|e| (e.path.as_str(), e.header_va)).collect();
        assert_eq!(got, [("a/x.luac", vas[0]), ("ui/y.xml", vas[1])]);
        assert_eq!(fs.read(&fs.entries()[0], &BuiltinCodecs).unwrap(), b"hello ".repeat(30));
        assert_eq!(fs.header(&fs.entries()[1]).unwrap().raw_size, 5);
    }

    #[test]
    fn truncated_entry_is_an_error_not_a_panic() {
        // As in the client's image: the header claims more compressed bytes than the file has,
        // and the bytes after its real end belong to the next object.
        let mut blob = lz4_blob(&[7u8; 5000], 0);
        let comp = u32::from_le_bytes(blob[32..36].try_into().unwrap()) + 100;
        blob[32..36].copy_from_slice(&comp.to_le_bytes());
        let stored = u64::from_le_bytes(blob[16..24].try_into().unwrap()) + 100;
        blob[16..24].copy_from_slice(&stored.to_le_bytes());
        blob.extend_from_slice(&[0x5A; 100]);
        let (pe, _) = exe_with_files(&[("big.luac", blob)]);
        let img = PeImage::parse(pe).unwrap();
        let fs = ExeFs::scan(&img).unwrap();
        assert!(fs.read(&fs.entries()[0], &BuiltinCodecs).is_err());
    }

    #[test]
    fn no_registry_is_an_error() {
        let img = PeImage::parse(pe_image(&[(".rdata", 0x1000, vec![0; 64]), (".data", 0x2000, vec![0; 64])])).unwrap();
        assert!(matches!(ExeFs::scan(&img), Err(Error::NoRegistry)));
    }
}
```

`src/re_dir.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir(t.path().join(".git")).unwrap();
        std::fs::create_dir(t.path().join("re")).unwrap();
        t
    }

    #[test]
    fn out_dir_inside_re_is_created() {
        let t = repo();
        let out = resolve_out_dir(t.path(), &t.path().join("re/m1/disasm")).unwrap();
        assert!(out.is_dir());
        assert!(out.starts_with(t.path().join("re").canonicalize().unwrap()));
    }

    #[test]
    fn out_dir_outside_re_is_refused() {
        let t = repo();
        assert!(resolve_out_dir(t.path(), &t.path().join("docs")).is_err());
        assert!(!t.path().join("docs").exists());
        assert!(resolve_out_dir(t.path(), &t.path().join("re")).is_err()); // a subdirectory, please
    }

    #[test]
    fn out_dir_with_dotdot_is_refused() {
        let t = repo();
        assert!(resolve_out_dir(t.path(), &t.path().join("re/../docs")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn out_dir_through_a_symlink_leaving_re_is_refused() {
        let t = repo();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), t.path().join("re/escape")).unwrap();
        assert!(resolve_out_dir(t.path(), &t.path().join("re/escape/x")).is_err());
        assert!(!outside.path().join("x").exists());
    }

    #[test]
    fn out_dir_without_re_is_refused() {
        let t = tempfile::tempdir().unwrap();
        assert!(resolve_out_dir(t.path(), &t.path().join("re/x")).is_err());
    }

    #[test]
    fn find_repo_root_walks_up() {
        let t = repo();
        std::fs::create_dir_all(t.path().join("a/b")).unwrap();
        assert_eq!(find_repo_root(&t.path().join("a/b")).unwrap(), t.path().canonicalize().unwrap());
    }

    #[test]
    fn safe_relative_accepts_plain_paths() {
        assert_eq!(safe_relative("addons/Login/Login.luac"), Some(PathBuf::from("addons/Login/Login.luac")));
    }

    #[test]
    fn safe_relative_refuses_escapes() {
        for bad in ["", "/etc/passwd", "a/../b", "..", "a//b", "a\\b", "C:/x", "a/./b", "a\nb"] {
            assert_eq!(safe_relative(bad), None, "{bad:?}");
        }
    }
}
```

- [ ] **Step 4: Run them and watch them fail.**

Run: `cargo test -p fenji-data`
Expected: compile errors: `PeImage`, `BlobHeader`, `decode`, `ExeFs`, `resolve_out_dir` … not found.

- [ ] **Step 5: Implement `pe.rs`**, above its test module:

```rust
//! The parts of a PE32+ image the readers need: its sections and the mapping from virtual
//! addresses to file bytes. Only file-backed bytes are readable, and nothing is executed.
use crate::{Error, Result};

/// One section of the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    /// Absolute virtual address: image base + RVA.
    pub va: u64,
    pub raw_offset: usize,
    pub raw_size: usize,
}

/// A parsed PE32+ image that owns its file bytes.
#[derive(Debug)]
pub struct PeImage {
    bytes: Vec<u8>,
    image_base: u64,
    sections: Vec<Section>,
}

fn le<const N: usize>(b: &[u8], at: usize) -> Option<[u8; N]> {
    b.get(at..at.checked_add(N)?)?.try_into().ok()
}

impl PeImage {
    /// Parses the DOS, COFF and optional headers and the section table.
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        if bytes.get(..2) != Some(b"MZ".as_slice()) {
            return Err(Error::NotPe("no MZ header"));
        }
        let pe = u32::from_le_bytes(le(&bytes, 0x3C).ok_or(Error::NotPe("truncated DOS header"))?) as usize;
        if bytes.get(pe..pe.saturating_add(4)) != Some(b"PE\0\0".as_slice()) {
            return Err(Error::NotPe("no PE signature"));
        }
        let count = u16::from_le_bytes(le(&bytes, pe + 6).ok_or(Error::NotPe("truncated COFF header"))?) as usize;
        let opt_size = u16::from_le_bytes(le(&bytes, pe + 20).ok_or(Error::NotPe("truncated COFF header"))?) as usize;
        let opt = pe + 24;
        if le::<2>(&bytes, opt).map(u16::from_le_bytes) != Some(0x20B) {
            return Err(Error::NotPe("not PE32+"));
        }
        let image_base = u64::from_le_bytes(le(&bytes, opt + 24).ok_or(Error::NotPe("truncated optional header"))?);
        let mut sections = Vec::with_capacity(count);
        for i in 0..count {
            let at = opt + opt_size + 40 * i;
            let raw = bytes.get(at..at + 40).ok_or(Error::NotPe("truncated section table"))?;
            let field = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().expect("4 bytes"));
            sections.push(Section {
                name: String::from_utf8_lossy(&raw[..8]).trim_end_matches('\0').to_string(),
                va: image_base + u64::from(field(12)),
                raw_size: field(16) as usize,
                raw_offset: field(20) as usize,
            });
        }
        Ok(Self { bytes, image_base, sections })
    }

    pub fn image_base(&self) -> u64 {
        self.image_base
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// The section whose file-backed bytes contain `va`.
    pub fn section_of(&self, va: u64) -> Option<&Section> {
        self.sections.iter().find(|s| va >= s.va && va - s.va < s.raw_size as u64)
    }

    /// A section's file bytes (clipped to the file).
    pub fn section_data(&self, s: &Section) -> &[u8] {
        let end = s.raw_offset.saturating_add(s.raw_size).min(self.bytes.len());
        self.bytes.get(s.raw_offset.min(end)..end).unwrap_or_default()
    }

    /// The file bytes behind `len` bytes at `va`; `None` unless all lie in one section's file data.
    pub fn read(&self, va: u64, len: usize) -> Option<&[u8]> {
        let s = self.section_of(va)?;
        let off = usize::try_from(va - s.va).ok()?;
        if off.checked_add(len)? > s.raw_size {
            return None;
        }
        let start = s.raw_offset.checked_add(off)?;
        self.bytes.get(start..start.checked_add(len)?)
    }

    /// The NUL-terminated string at `va` (without its NUL), at most `max` bytes long.
    pub fn c_str(&self, va: u64, max: usize) -> Option<&[u8]> {
        let s = self.section_of(va)?;
        let off = usize::try_from(va - s.va).ok()?;
        let data = self.section_data(s).get(off..)?;
        let end = data.iter().take(max.saturating_add(1)).position(|&b| b == 0)?;
        Some(&data[..end])
    }
}
```

- [ ] **Step 6: Implement `blob.rs` and `codec.rs`.**

`src/blob.rs`, above its test module:

```rust
//! The engine's file blob. The same header fronts every file in the data archives and in the
//! file system embedded in gujianol.exe (docs/re/m1-script-format.md §2).
//!
//! Layout (little-endian): raw_size u64, timestamp u64 (Unix seconds), stored_size u64,
//! block_size u32, codec u16, check u16; then one u32 compressed size per block for compressed
//! codecs; then the data. stored_size counts from the header's first byte to the data's last.
use crate::{Decompressor, Error, Result};

/// Length of the fixed part of the header.
pub const FIXED_LEN: usize = 32;
/// No client file comes near this; larger raw sizes mean a corrupt header.
const MAX_RAW: u64 = 1 << 32;

/// How a blob's data is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Stored,
    Lz4,
    Oodle,
    Other(u16),
}

impl Codec {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0 => Self::Stored,
            1 => Self::Lz4,
            4 => Self::Oodle,
            other => Self::Other(other),
        }
    }

    pub fn as_u16(self) -> u16 {
        match self {
            Self::Stored => 0,
            Self::Lz4 => 1,
            Self::Oodle => 4,
            Self::Other(v) => v,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Lz4 => "lz4",
            Self::Oodle => "oodle",
            Self::Other(_) => "unknown",
        }
    }
}

/// A parsed blob header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobHeader {
    pub raw_size: u64,
    pub timestamp: u64,
    pub stored_size: u64,
    pub block_size: u32,
    pub codec: Codec,
    /// Meaning unknown (docs/re/m1-script-format.md §2).
    pub check: u16,
    /// Compressed size of each block; empty for stored blobs.
    pub block_sizes: Vec<u32>,
}

impl BlobHeader {
    /// Parses the header at the start of `bytes`. `at` only labels errors. Refuses a header whose
    /// stored size disagrees with its own block list.
    pub fn parse(bytes: &[u8], at: u64) -> Result<Self> {
        let bad = |reason: String| Error::BadBlob { at, reason };
        let fixed = bytes
            .get(..FIXED_LEN)
            .ok_or_else(|| bad(format!("{} bytes, the header needs {FIXED_LEN}", bytes.len())))?;
        let u64_at = |o: usize| u64::from_le_bytes(fixed[o..o + 8].try_into().expect("8 bytes"));
        let (raw_size, timestamp, stored_size) = (u64_at(0), u64_at(8), u64_at(16));
        let block_size = u32::from_le_bytes(fixed[24..28].try_into().expect("4 bytes"));
        let codec = Codec::from_u16(u16::from_le_bytes([fixed[28], fixed[29]]));
        let check = u16::from_le_bytes([fixed[30], fixed[31]]);
        if raw_size > MAX_RAW {
            return Err(bad(format!("raw size {raw_size} is implausible")));
        }
        let blocks = match codec {
            Codec::Stored => 0,
            _ if raw_size == 0 => 0,
            _ if block_size == 0 => return Err(bad("block size 0 with data".into())),
            _ => raw_size.div_ceil(u64::from(block_size)) as usize,
        };
        let list = bytes
            .get(FIXED_LEN..FIXED_LEN + 4 * blocks)
            .ok_or_else(|| bad(format!("the list of {blocks} block sizes is truncated")))?;
        let block_sizes = list
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes")))
            .collect();
        let header = Self { raw_size, timestamp, stored_size, block_size, codec, check, block_sizes };
        let expected = header.header_len() as u64 + header.data_len();
        if expected != stored_size {
            return Err(bad(format!("stored size {stored_size}, but header and blocks add up to {expected}")));
        }
        Ok(header)
    }

    /// Length of the header including the block list.
    pub fn header_len(&self) -> usize {
        FIXED_LEN + 4 * self.block_sizes.len()
    }

    /// Length of the data after the header.
    pub fn data_len(&self) -> u64 {
        match self.codec {
            Codec::Stored => self.raw_size,
            _ => self.block_sizes.iter().map(|&s| u64::from(s)).sum(),
        }
    }
}

/// Decodes a whole blob. `bytes` starts at its header and holds at least `stored_size` bytes.
pub fn decode(bytes: &[u8], at: u64, codecs: &dyn Decompressor) -> Result<Vec<u8>> {
    let h = BlobHeader::parse(bytes, at)?;
    let data = bytes.get(h.header_len()..h.stored_size as usize).ok_or_else(|| Error::BadBlob {
        at,
        reason: format!("{} bytes available, stored size {}", bytes.len(), h.stored_size),
    })?;
    if h.codec == Codec::Stored {
        return codecs.decompress_block(Codec::Stored, data, h.raw_size as usize, 0);
    }
    let mut out = Vec::with_capacity(h.raw_size as usize);
    let mut pos = 0usize;
    for (index, &len) in h.block_sizes.iter().enumerate() {
        let raw_len = (h.raw_size - out.len() as u64).min(u64::from(h.block_size)) as usize;
        // In range: `data` is exactly the sum of the block sizes (checked by `parse`).
        let block = &data[pos..pos + len as usize];
        out.extend_from_slice(&codecs.decompress_block(h.codec, block, raw_len, index)?);
        pos += len as usize;
    }
    if out.len() as u64 != h.raw_size {
        return Err(Error::SizeMismatch { got: out.len() as u64, want: h.raw_size });
    }
    Ok(out)
}
```

`src/codec.rs`:

```rust
//! Block decompression. Fenji decodes stored and LZ4 blocks itself; Oodle (codec 4) waits on the
//! M1 decision gate and is refused until then.
use crate::{Codec, Error, Result};

/// Decodes one block of a blob.
pub trait Decompressor {
    /// Decodes `input`, one block of `codec` data, into exactly `raw_len` bytes. `index` is the
    /// block's position in its blob, for messages.
    fn decompress_block(&self, codec: Codec, input: &[u8], raw_len: usize, index: usize) -> Result<Vec<u8>>;
}

/// Stored and LZ4.
#[derive(Debug, Default, Clone, Copy)]
pub struct BuiltinCodecs;

impl Decompressor for BuiltinCodecs {
    fn decompress_block(&self, codec: Codec, input: &[u8], raw_len: usize, index: usize) -> Result<Vec<u8>> {
        let out = match codec {
            Codec::Stored => input.to_vec(),
            Codec::Lz4 => lz4_flex::block::decompress(input, raw_len)
                .map_err(|e| Error::Lz4 { index, reason: e.to_string() })?,
            other => return Err(Error::CodecUnavailable(other.as_u16())),
        };
        if out.len() != raw_len {
            return Err(Error::SizeMismatch { got: out.len() as u64, want: raw_len as u64 });
        }
        Ok(out)
    }
}
```

- [ ] **Step 7: Implement `exe_fs.rs` and `re_dir.rs`.**

`src/exe_fs.rs`, above its test module:

```rust
//! The file system embedded in gujianol.exe: scripts, UI XML and configs packed as blobs in
//! `.rdata`, listed by registry tables in `.data` (docs/re/m1-script-format.md §1).
use crate::{blob, BlobHeader, Decompressor, Error, PeImage, Result};

/// The longest path the scanner accepts as a registry name.
const MAX_PATH: usize = 260;

/// One file of the embedded file system, as the registry lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExeEntry {
    pub path: String,
    pub stored_size: u64,
    /// Virtual address of the file's blob header.
    pub header_va: u64,
}

/// The embedded file system of one image.
#[derive(Debug)]
pub struct ExeFs<'a> {
    image: &'a PeImage,
    entries: Vec<ExeEntry>,
}

impl<'a> ExeFs<'a> {
    /// Finds every registry triple in `.data` (8-byte aligned): a pointer to a NUL-terminated path
    /// in `.rdata`, a stored size, and a pointer to a blob header in `.rdata` that agrees with it.
    pub fn scan(image: &'a PeImage) -> Result<Self> {
        let data = image.section(".data").ok_or(Error::NoRegistry)?;
        let bytes = image.section_data(data);
        let mut entries = Vec::new();
        let mut off = 0;
        while off + 24 <= bytes.len() {
            if let Some(entry) = triple(image, &bytes[off..off + 24]) {
                entries.push(entry);
                off += 24;
            } else {
                off += 8;
            }
        }
        if entries.is_empty() {
            return Err(Error::NoRegistry);
        }
        Ok(Self { image, entries })
    }

    /// Every registry entry, in registry order.
    pub fn entries(&self) -> &[ExeEntry] {
        &self.entries
    }

    /// The entry's blob header.
    pub fn header(&self, e: &ExeEntry) -> Result<BlobHeader> {
        BlobHeader::parse(self.stored_bytes(e)?, e.header_va)
    }

    /// The entry's decoded content.
    pub fn read(&self, e: &ExeEntry, codecs: &dyn Decompressor) -> Result<Vec<u8>> {
        blob::decode(self.stored_bytes(e)?, e.header_va, codecs)
    }

    fn stored_bytes(&self, e: &ExeEntry) -> Result<&'a [u8]> {
        let missing = || Error::OutOfImage { va: e.header_va, len: e.stored_size };
        let len = usize::try_from(e.stored_size).map_err(|_| missing())?;
        self.image.read(e.header_va, len).ok_or_else(missing)
    }
}

fn triple(image: &PeImage, t: &[u8]) -> Option<ExeEntry> {
    let word = |i: usize| u64::from_le_bytes(t[8 * i..8 * i + 8].try_into().expect("8 bytes"));
    let (path_va, stored, header_va) = (word(0), word(1), word(2));
    if !(blob::FIXED_LEN as u64..=u64::from(u32::MAX)).contains(&stored) {
        return None;
    }
    if image.section_of(path_va)?.name != ".rdata" || image.section_of(header_va)?.name != ".rdata" {
        return None;
    }
    let name = image.c_str(path_va, MAX_PATH)?;
    if name.is_empty() || name.iter().any(|&b| b < 0x20 || b == 0x7F) {
        return None;
    }
    let path = std::str::from_utf8(name).ok()?.to_owned();
    let header = BlobHeader::parse(image.read(header_va, stored as usize)?, header_va).ok()?;
    (header.stored_size == stored).then_some(ExeEntry { path, stored_size: stored, header_va })
}
```

`src/re_dir.rs`, above its test module:

```rust
//! Where tools may write: only inside the repository's git-ignored `re/` (ground rule 3).
use std::path::{Component, Path, PathBuf};

use crate::{Error, Result};

/// The repository root: the nearest ancestor of `start` holding both `.git` and a `re/` directory.
pub fn find_repo_root(start: &Path) -> Result<PathBuf> {
    let start = start.canonicalize()?;
    start
        .ancestors()
        .find(|d| d.join(".git").exists() && d.join("re").is_dir())
        .map(Path::to_path_buf)
        .ok_or_else(|| Error::OutDir(format!("no Fenji checkout with a re/ directory above {}", start.display())))
}

/// Resolves `requested` (absolute, or relative to the current directory) to a directory strictly
/// inside `<repo_root>/re/`, and creates it. Refuses `..`, `re/` itself, and anything that
/// resolves outside `re/`, including through a symlink.
pub fn resolve_out_dir(repo_root: &Path, requested: &Path) -> Result<PathBuf> {
    let re = repo_root.join("re").canonicalize()?;
    let abs = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        std::env::current_dir()?.join(requested)
    };
    if abs.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Error::OutDir(format!("{} contains '..'", requested.display())));
    }
    let no_ancestor = || Error::OutDir(format!("{} has no existing ancestor", abs.display()));
    // Canonicalise the longest existing ancestor, then add back the parts that don't exist yet.
    let (mut existing, mut rest) = (abs.as_path(), Vec::new());
    while !existing.exists() {
        rest.push(existing.file_name().ok_or_else(no_ancestor)?.to_owned());
        existing = existing.parent().ok_or_else(no_ancestor)?;
    }
    let mut out = existing.canonicalize()?;
    out.extend(rest.iter().rev());
    if out == re || !out.starts_with(&re) {
        return Err(Error::OutDir(format!("{} is not inside {}", out.display(), re.display())));
    }
    std::fs::create_dir_all(&out)?;
    Ok(out)
}

/// A registry path as a relative path that can't leave the directory it is joined to: plain
/// components only, with no empty part, `.`, `..`, backslash, drive colon or control character.
pub fn safe_relative(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.contains(['\\', ':'])
            || part.chars().any(char::is_control)
        {
            return None;
        }
        out.push(part);
    }
    Some(out)
}
```

- [ ] **Step 8: Run the tests until they pass, then the full gate.**

Run: `cargo test -p fenji-data`
Expected: `test result: ok. 20 passed`: 1 lib, 3 pe, 5 blob, 3 exe_fs and 8 re_dir (one of those is the Unix-only symlink test).

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo deny check`
Expected: all pass; `cargo deny` prints `licenses ok`.

- [ ] **Step 9: Commit, PR and merge.**

```bash
scripts/check-forbidden-files.sh --staged
git add Cargo.toml Cargo.lock crates/fenji-data
git commit -m "feat(data): read the client's blob format and its embedded file system" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

This task carries a security boundary (the `re/` guard, Review Focus 3). Before merging, dispatch one fresh reviewer (`general-purpose`, `opus`) over the PR diff. Its brief: the global constraints, Review Focus 1 and 3, and "try to make `resolve_out_dir` or `safe_relative` let a write escape `re/`". Fix what it finds, then wait for CI and squash-merge.

---

### Task 4: `fenji-lua`: opcodes, chunk model, parse and write in both dialects

**Files:**
- Modify: `Cargo.toml` (members, dependencies), `Cargo.lock`, `docs/re/sources.md`
- Create:
  - `crates/fenji-lua/Cargo.toml`
  - `crates/fenji-lua/src/lib.rs`
  - `crates/fenji-lua/src/error.rs`
  - `crates/fenji-lua/src/opcode.rs`
  - `crates/fenji-lua/src/chunk.rs`
  - `crates/fenji-lua/tests/common/mod.rs`
  - `crates/fenji-lua/tests/chunk.rs`

**Interfaces:**
- Consumes Task 1 §3: the header bytes and the opcode numbering.
- Produces:
  - `fenji_lua::Dialect::{Stock53, Client}`, with `Dialect::detect(&[u8]) -> Option<Dialect>`
  - `fenji_lua::header(Dialect) -> Vec<u8>`
  - `fenji_lua::parse(&[u8], Dialect) -> Result<Chunk>` and `fenji_lua::write(&Chunk, Dialect) -> Vec<u8>`
  - `Chunk { upvalue_count: u8, main: Function }`
  - `Function`, with public fields `source`, `line_defined`, `last_line_defined`, `num_params`, `is_vararg`, `max_stack_size`, `code: Vec<u32>`, `constants: Vec<Constant>`, `upvalues`, `protos: Vec<Function>`, `line_info`, `loc_vars` and `upvalue_names`
  - `Constant::{Nil, Boolean, Float, Integer, ShortString(Vec<u8>), LongString(Vec<u8>)}`
  - `fenji_lua::Op` (54 variants; `from_code`, `code`, `is_stock`, `name`, `mode`, `rk_operands`) and `fenji_lua::Mode`
  - `fenji_lua::Instr(u32)` (`op`, `code`, `a`, `b`, `c`, `bx`, `sbx`, `ax`, `with_op`), plus `fenji_lua::{is_k, k_index, BITRK, STOCK_COUNT}`
  - `fenji_lua::Error`, which derives `PartialEq`, and `fenji_lua::Result`

- [ ] **Step 1: Branch, workspace entries and manifest.**

```bash
git switch dev && git pull --ff-only && git switch -c feat/lua-<issue>-chunks
```

Root `Cargo.toml`:
- add `"crates/fenji-lua"` to `members`;
- add `fenji-lua = { path = "crates/fenji-lua" }` and `mlua = { version = "0.12.2", features = ["lua53", "vendored"] }` to `[workspace.dependencies]`, keeping alphabetical order.

`crates/fenji-lua/Cargo.toml`:

```toml
[package]
name = "fenji-lua"
description = "The SoLO client's Lua 5.3 dialect: chunk parser and writer, disassembler, normaliser to stock Lua 5.3, script census"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
thiserror.workspace = true

[dev-dependencies]
# Real Lua 5.3, compiled from source by the build, so tests make their own fixtures from our own
# Lua code at test time. Bytecode is never committed.
mlua.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write the test fixtures and the failing tests.**

`crates/fenji-lua/tests/common/mod.rs`:

```rust
//! Test fixtures compiled at test time by real Lua 5.3 (mlua, vendored) from our own source, so
//! no bytecode is ever committed (ground rule 3; the forbidden-file check rejects it anyway).
#![allow(dead_code)]

/// A module that touches every constant type, closures and upvalues, numeric and generic `for`,
/// varargs, methods, integer division, `goto`, and a global call guarded by `IsServer()`.
pub const MODULE: &str = r#"local M = {}
local server = { host = "127.0.0.1", port = 7804, ratio = 0.5, on = true, off = false, none = nil }
function M.pick(list, name)
  for i, s in ipairs(list) do
    if s.name == name then return s, i end
  end
  return nil
end
function M:connect(opts)
  local h = opts and opts.host or server.host
  if IsServer() then return string.format("%s:%d", h, server.port) end
  return h
end
local n = 0
for i = 1, 10, 2 do n = n + i // 2 end
local function count(...) return select('#', ...) end
M.count = count
goto done
::done::
return M
"#;

/// Our test sources: the module, a one-liner, a long string (over 254 bytes, so its length is
/// written as 0xFF plus a size_t), and a generic loop over `_G`.
pub fn sources() -> Vec<String> {
    vec![
        MODULE.to_string(),
        "print('hello')".to_string(),
        format!("local s = \"{}\" return s, #s", "x".repeat(300)),
        "local t = {} for k, v in pairs(_G) do t[k] = type(v) end return t".to_string(),
    ]
}

/// `src` compiled by Lua 5.3 and dumped: stripped (as all of the client's chunks are) or not.
pub fn luac(src: &str, strip: bool) -> Vec<u8> {
    let lua = mlua::Lua::new();
    lua.load(src).into_function().expect("our test source compiles").dump(strip)
}

/// Every source, both stripped and with debug info.
pub fn fixtures() -> Vec<Vec<u8>> {
    sources().iter().flat_map(|s| [luac(s, true), luac(s, false)]).collect()
}
```

`crates/fenji-lua/tests/chunk.rs`:

```rust
mod common;

use fenji_lua::{header, parse, write, Chunk, Dialect, Error, Function};

#[test]
fn stock_round_trip_is_byte_exact() {
    for bytes in common::fixtures() {
        let chunk = parse(&bytes, Dialect::Stock53).expect("a stock chunk parses");
        assert_eq!(write(&chunk, Dialect::Stock53), bytes);
    }
}

#[test]
fn client_dialect_is_stock_without_the_size_t_byte() {
    for bytes in common::fixtures() {
        let chunk = parse(&bytes, Dialect::Stock53).unwrap();
        let client = write(&chunk, Dialect::Client);
        let mut expected = bytes.clone();
        expected.remove(13); // sizes: int, size_t, Instruction, lua_Integer, lua_Number
        assert_eq!(client, expected);
        assert_eq!(parse(&client, Dialect::Client).unwrap(), chunk);
        assert_eq!(Dialect::detect(&client), Some(Dialect::Client));
        assert_eq!(Dialect::detect(&bytes), Some(Dialect::Stock53));
    }
}

#[test]
fn the_wrong_dialect_is_refused() {
    let bytes = common::luac("print('hello')", true);
    assert_eq!(parse(&bytes, Dialect::Client), Err(Error::Header(Dialect::Client)));
}

#[test]
fn stripped_fixtures_look_like_the_clients() {
    let chunk = parse(&common::luac(common::MODULE, true), Dialect::Stock53).unwrap();
    assert!(chunk.main.source.is_none());
    assert!(chunk.main.line_info.is_empty() && chunk.main.loc_vars.is_empty());
}

#[test]
fn every_truncation_is_an_error() {
    let chunk = parse(&common::luac(common::MODULE, true), Dialect::Stock53).unwrap();
    let bytes = write(&chunk, Dialect::Client);
    for n in 0..bytes.len() {
        assert!(parse(&bytes[..n], Dialect::Client).is_err(), "prefix of {n} bytes");
    }
}

#[test]
fn trailing_bytes_are_refused() {
    let mut bytes = common::luac("print('hello')", true);
    bytes.push(0);
    assert!(matches!(parse(&bytes, Dialect::Stock53), Err(Error::Trailing { extra: 1, .. })));
}

/// Byte offset of the main function's instruction count in a stripped stock chunk: the header, the
/// upvalue count, an empty source (1 byte), two line numbers, then three bytes.
fn code_count_at() -> usize {
    header(Dialect::Stock53).len() + 1 + 1 + 8 + 3
}

#[test]
fn absurd_count_is_refused() {
    let mut bytes = common::luac("print('hello')", true);
    let at = code_count_at();
    bytes[at..at + 4].copy_from_slice(&0x7FFF_FFFFi32.to_le_bytes());
    assert!(matches!(parse(&bytes, Dialect::Stock53), Err(Error::BadCount { .. })));
}

#[test]
fn unknown_constant_tag_is_refused() {
    let bytes = common::luac("return 'x'", true);
    let chunk = parse(&bytes, Dialect::Stock53).unwrap();
    let tag_at = code_count_at() + 4 + 4 * chunk.main.code.len() + 4; // count, code, constant count
    assert_eq!(bytes[tag_at], 4, "the first constant is the short string 'x'");
    let mut bad = bytes.clone();
    bad[tag_at] = 7;
    assert!(matches!(parse(&bad, Dialect::Stock53), Err(Error::ConstantType { tag: 7, .. })));
}

#[test]
fn deep_nesting_is_refused() {
    let mut f = Function::default();
    for _ in 0..300 {
        f = Function { protos: vec![f], ..Function::default() };
    }
    let bytes = write(&Chunk { upvalue_count: 0, main: f }, Dialect::Stock53);
    assert!(matches!(parse(&bytes, Dialect::Stock53), Err(Error::TooDeep { .. })));
}
```

Run: `cargo test -p fenji-lua`
Expected: compile errors (`fenji_lua::parse` and others not found).

- [ ] **Step 3: Write `src/lib.rs`, `src/error.rs` and `src/opcode.rs`** (the opcode unit tests are included).

`crates/fenji-lua/src/lib.rs`:

```rust
//! The SoLO client's Lua 5.3 dialect (M1 spec §3): binary chunks in the stock and the client
//! dialect, a parser and a writer; later a disassembler, a normaliser to stock Lua 5.3 and a
//! census. Pure: no I/O.
mod chunk;
mod error;
mod opcode;

pub use chunk::{header, parse, write, Chunk, Constant, Dialect, Function, LocVar, Upvalue, MAX_DEPTH};
pub use error::{Error, Result};
pub use opcode::{is_k, k_index, Instr, Mode, Op, BITRK, STOCK_COUNT};
```

`crates/fenji-lua/src/error.rs`:

```rust
use crate::Dialect;

/// What can go wrong with a chunk.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("not a {0:?} Lua 5.3 header")]
    Header(Dialect),
    #[error("truncated at byte {offset}: {needed} more bytes needed")]
    Truncated { offset: usize, needed: usize },
    #[error("implausible count {count} at byte {offset}")]
    BadCount { offset: usize, count: i64 },
    #[error("unknown constant type {tag} at byte {offset}")]
    ConstantType { offset: usize, tag: u8 },
    #[error("NULL string constant at byte {offset}")]
    NullString { offset: usize },
    #[error("{extra} bytes after the main function (from byte {offset})")]
    Trailing { offset: usize, extra: usize },
    #[error("functions nested deeper than {limit} (at byte {offset})")]
    TooDeep { offset: usize, limit: usize },
    #[error("opcode {code} is not in the client's table")]
    UnknownOpcode { code: u8 },
    #[error("{op} has no established stock equivalent (docs/re/m1-script-format.md §4)")]
    Unmapped { op: &'static str },
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
```

`crates/fenji-lua/src/opcode.rs`:

```rust
//! Lua 5.3's opcodes and the seven the client's VM adds. The client keeps the stock numbers 0–46
//! (each of its 89,127 functions ends in 38, RETURN) and adds 47–53 (spike-0 6.17;
//! docs/re/m1-script-format.md §3–§4).

/// How an instruction's operands are packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Abc,
    Abx,
    AsBx,
    Ax,
}

/// One opcode. Declaration order is the client's numbering, so `op as u8` is its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Op {
    Move, LoadK, LoadKx, LoadBool, LoadNil, GetUpval, GetTabUp, GetTable, SetTabUp, SetUpval,
    SetTable, NewTable, SelfOp, Add, Sub, Mul, Mod, Pow, Div, IDiv, BAnd, BOr, BXor, Shl, Shr,
    Unm, BNot, Not, Len, Concat, Jmp, Eq, Lt, Le, Test, TestSet, Call, TailCall, Return,
    ForLoop, ForPrep, TForCall, TForLoop, SetList, Closure, VarArg, ExtraArg,
    // Client-only, 47–53.
    Nop, XGetTabUp, TryTabUp, TryLibUp, XGetSelf, XSelf, TrySelf,
}

const ALL: [Op; 54] = [
    Op::Move, Op::LoadK, Op::LoadKx, Op::LoadBool, Op::LoadNil, Op::GetUpval, Op::GetTabUp,
    Op::GetTable, Op::SetTabUp, Op::SetUpval, Op::SetTable, Op::NewTable, Op::SelfOp, Op::Add,
    Op::Sub, Op::Mul, Op::Mod, Op::Pow, Op::Div, Op::IDiv, Op::BAnd, Op::BOr, Op::BXor, Op::Shl,
    Op::Shr, Op::Unm, Op::BNot, Op::Not, Op::Len, Op::Concat, Op::Jmp, Op::Eq, Op::Lt, Op::Le,
    Op::Test, Op::TestSet, Op::Call, Op::TailCall, Op::Return, Op::ForLoop, Op::ForPrep,
    Op::TForCall, Op::TForLoop, Op::SetList, Op::Closure, Op::VarArg, Op::ExtraArg, Op::Nop,
    Op::XGetTabUp, Op::TryTabUp, Op::TryLibUp, Op::XGetSelf, Op::XSelf, Op::TrySelf,
];

/// The names in the client's own opcode-name table (spike-0 6.17), in code order.
const NAMES: [&str; 54] = [
    "MOVE", "LOADK", "LOADKX", "LOADBOOL", "LOADNIL", "GETUPVAL", "GETTABUP", "GETTABLE",
    "SETTABUP", "SETUPVAL", "SETTABLE", "NEWTABLE", "SELF", "ADD", "SUB", "MUL", "MOD", "POW",
    "DIV", "IDIV", "BAND", "BOR", "BXOR", "SHL", "SHR", "UNM", "BNOT", "NOT", "LEN", "CONCAT",
    "JMP", "EQ", "LT", "LE", "TEST", "TESTSET", "CALL", "TAILCALL", "RETURN", "FORLOOP",
    "FORPREP", "TFORCALL", "TFORLOOP", "SETLIST", "CLOSURE", "VARARG", "EXTRAARG", "NOP",
    "XGETTABUP", "TRYTABUP", "TRYLIBUP", "XGETSELF", "XSELF", "TRYSELF",
];

/// The number of stock Lua 5.3 opcodes.
pub const STOCK_COUNT: u8 = 47;

impl Op {
    /// The opcode whose client code is `code` (0–53).
    pub fn from_code(code: u8) -> Option<Op> {
        ALL.get(usize::from(code)).copied()
    }

    /// The client's code; equal to the stock code for 0–46.
    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn is_stock(self) -> bool {
        self.code() < STOCK_COUNT
    }

    /// The name the client's opcode table uses.
    pub fn name(self) -> &'static str {
        NAMES[usize::from(self.code())]
    }

    pub fn mode(self) -> Mode {
        match self {
            Op::LoadK | Op::LoadKx | Op::Closure => Mode::Abx,
            Op::Jmp | Op::ForLoop | Op::ForPrep | Op::TForLoop => Mode::AsBx,
            Op::ExtraArg => Mode::Ax,
            _ => Mode::Abc,
        }
    }

    /// Whether B and C are RK operands: a register, or a constant when bit 8 is set. The client's
    /// extras follow their stock namesakes (planning probe; docs/re/m1-script-format.md §4).
    pub fn rk_operands(self) -> (bool, bool) {
        match self {
            Op::GetTabUp | Op::GetTable | Op::SelfOp | Op::XGetTabUp | Op::TryTabUp | Op::TryLibUp
            | Op::XGetSelf | Op::XSelf | Op::TrySelf => (false, true),
            Op::SetTabUp | Op::SetTable | Op::Add | Op::Sub | Op::Mul | Op::Mod | Op::Pow | Op::Div
            | Op::IDiv | Op::BAnd | Op::BOr | Op::BXor | Op::Shl | Op::Shr | Op::Eq | Op::Lt
            | Op::Le => (true, true),
            _ => (false, false),
        }
    }
}

/// Bit 8 of a B or C operand marks a constant index (Lua 5.3's BITRK).
pub const BITRK: u32 = 1 << 8;

pub fn is_k(x: u32) -> bool {
    x & BITRK != 0
}

pub fn k_index(x: u32) -> u32 {
    x & !BITRK
}

/// One instruction, in Lua 5.3's layout: op 6 bits, A 8, C 9, B 9; Bx 18; sBx biased by 131,071.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instr(pub u32);

impl Instr {
    pub fn code(self) -> u8 {
        (self.0 & 0x3F) as u8
    }

    pub fn op(self) -> Option<Op> {
        Op::from_code(self.code())
    }

    pub fn a(self) -> u32 {
        (self.0 >> 6) & 0xFF
    }

    pub fn c(self) -> u32 {
        (self.0 >> 14) & 0x1FF
    }

    pub fn b(self) -> u32 {
        (self.0 >> 23) & 0x1FF
    }

    pub fn bx(self) -> u32 {
        self.0 >> 14
    }

    pub fn sbx(self) -> i32 {
        (self.0 >> 14) as i32 - 131_071
    }

    pub fn ax(self) -> u32 {
        self.0 >> 6
    }

    /// The same operands under another opcode.
    pub fn with_op(self, op: Op) -> Instr {
        Instr((self.0 & !0x3F) | u32::from(op.code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_names_line_up() {
        for code in 0..54u8 {
            let op = Op::from_code(code).unwrap();
            assert_eq!(op.code(), code);
            assert_eq!(op.is_stock(), code < 47);
        }
        assert_eq!(Op::from_code(54), None);
        assert_eq!((Op::Return.code(), Op::Return.name()), (38, "RETURN"));
        assert_eq!((Op::TryTabUp.code(), Op::TrySelf.name()), (49, "TRYSELF"));
    }

    #[test]
    fn instruction_fields() {
        let ret = Instr(38 | (1 << 23)); // RETURN 0 1: how every Lua 5.3 function ends
        assert_eq!((ret.op(), ret.a(), ret.b(), ret.c()), (Some(Op::Return), 0, 1, 0));
        assert_eq!(Instr(30 | ((131_071 - 1) << 14)).sbx(), -1); // JMP 0 -1
        assert_eq!(Instr(38).with_op(Op::Nop).code(), 47);
        assert!(is_k(BITRK | 3) && k_index(BITRK | 3) == 3 && !is_k(3));
    }
}
```

- [ ] **Step 4: Implement `src/chunk.rs`.**

```rust
//! Lua 5.3 binary chunks in two dialects: stock (`luac` 5.3), and the client's, whose header
//! leaves out the size_t size byte while the dump still writes 8-byte size_t string lengths
//! (spike-0 6.15; docs/re/m1-script-format.md §3).
use crate::{Error, Result};

/// The two chunk dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Stock53,
    Client,
}

const SIGNATURE: &[u8] = b"\x1bLua";
const LUAC_DATA: &[u8] = b"\x19\x93\r\n\x1a\n";
const LUAC_INT: i64 = 0x5678;
const LUAC_NUM: f64 = 370.5;
/// Deeper nesting is refused: real chunks are shallow, and the recursion must stay bounded.
pub const MAX_DEPTH: usize = 200;

impl Dialect {
    fn sizes(self) -> &'static [u8] {
        match self {
            // int, size_t, Instruction, lua_Integer, lua_Number
            Self::Stock53 => &[4, 8, 4, 8, 8],
            // the same without size_t
            Self::Client => &[4, 4, 8, 8],
        }
    }

    /// The dialect whose header `bytes` starts with, if either.
    pub fn detect(bytes: &[u8]) -> Option<Self> {
        [Self::Stock53, Self::Client].into_iter().find(|d| bytes.starts_with(&header(*d)))
    }
}

/// A dialect's exact header bytes.
pub fn header(d: Dialect) -> Vec<u8> {
    let mut h = Vec::with_capacity(33);
    h.extend_from_slice(SIGNATURE);
    h.extend_from_slice(&[0x53, 0]); // version 5.3, official format
    h.extend_from_slice(LUAC_DATA);
    h.extend_from_slice(d.sizes());
    h.extend_from_slice(&LUAC_INT.to_le_bytes());
    h.extend_from_slice(&LUAC_NUM.to_le_bytes());
    h
}

/// A parsed chunk: the main function and its upvalue count.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub upvalue_count: u8,
    pub main: Function,
}

/// One function prototype, in dump order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Function {
    pub source: Option<Vec<u8>>,
    pub line_defined: i32,
    pub last_line_defined: i32,
    pub num_params: u8,
    pub is_vararg: u8,
    pub max_stack_size: u8,
    pub code: Vec<u32>,
    pub constants: Vec<Constant>,
    pub upvalues: Vec<Upvalue>,
    pub protos: Vec<Function>,
    pub line_info: Vec<i32>,
    pub loc_vars: Vec<LocVar>,
    pub upvalue_names: Vec<Option<Vec<u8>>>,
}

/// A constant, with Lua 5.3's type tags: 0 nil, 1 boolean, 3 float, 19 integer, 4 short string,
/// 20 long string.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Nil,
    Boolean(bool),
    Float(f64),
    Integer(i64),
    ShortString(Vec<u8>),
    LongString(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Upvalue {
    pub in_stack: u8,
    pub index: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocVar {
    pub name: Option<Vec<u8>>,
    pub start_pc: i32,
    pub end_pc: i32,
}

/// Parses a whole chunk of `dialect`, refusing anything left over after the main function.
pub fn parse(bytes: &[u8], dialect: Dialect) -> Result<Chunk> {
    let h = header(dialect);
    if !bytes.starts_with(&h) {
        return Err(Error::Header(dialect));
    }
    let mut r = Reader { b: bytes, p: h.len() };
    let upvalue_count = r.u8()?;
    let main = r.function(0)?;
    if r.p != bytes.len() {
        return Err(Error::Trailing { offset: r.p, extra: bytes.len() - r.p });
    }
    Ok(Chunk { upvalue_count, main })
}

/// Writes `chunk` in `dialect`. For every chunk `parse` accepts, `write(&parse(b, d)?, d) == b`.
pub fn write(chunk: &Chunk, dialect: Dialect) -> Vec<u8> {
    let mut w = header(dialect);
    w.push(chunk.upvalue_count);
    write_function(&mut w, &chunk.main);
    w
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or(Error::Truncated { offset: self.p, needed: n })?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }

    /// A count followed by items of at least `min_item` bytes each. Refuses counts the rest of
    /// the input can't hold, so a corrupt count never causes a huge allocation.
    fn count(&mut self, min_item: usize) -> Result<usize> {
        let at = self.p;
        let n = self.i32()?;
        let bad = Error::BadCount { offset: at, count: i64::from(n) };
        let n = usize::try_from(n).map_err(|_| bad.clone())?;
        if n.saturating_mul(min_item) > self.b.len() - self.p {
            return Err(bad);
        }
        Ok(n)
    }

    /// A string: one size byte (length + 1), or 0xFF and an 8-byte size_t; size 0 is NULL.
    fn string(&mut self) -> Result<Option<Vec<u8>>> {
        let at = self.p;
        let mut size = u64::from(self.u8()?);
        if size == 0xFF {
            size = self.u64()?;
        }
        if size == 0 {
            return Ok(None);
        }
        let len = usize::try_from(size - 1).map_err(|_| Error::BadCount { offset: at, count: size as i64 })?;
        Ok(Some(self.take(len)?.to_vec()))
    }

    fn function(&mut self, depth: usize) -> Result<Function> {
        if depth > MAX_DEPTH {
            return Err(Error::TooDeep { offset: self.p, limit: MAX_DEPTH });
        }
        let mut f = Function {
            source: self.string()?,
            line_defined: self.i32()?,
            last_line_defined: self.i32()?,
            num_params: self.u8()?,
            is_vararg: self.u8()?,
            max_stack_size: self.u8()?,
            ..Function::default()
        };
        let n = self.count(4)?;
        f.code = (0..n).map(|_| self.u32()).collect::<Result<_>>()?;
        let n = self.count(1)?;
        for _ in 0..n {
            let at = self.p;
            let constant = match self.u8()? {
                0 => Constant::Nil,
                1 => Constant::Boolean(self.u8()? != 0),
                3 => Constant::Float(f64::from_bits(self.u64()?)),
                19 => Constant::Integer(self.u64()? as i64),
                tag @ (4 | 20) => {
                    let s = self.string()?.ok_or(Error::NullString { offset: at })?;
                    if tag == 4 { Constant::ShortString(s) } else { Constant::LongString(s) }
                }
                tag => return Err(Error::ConstantType { offset: at, tag }),
            };
            f.constants.push(constant);
        }
        let n = self.count(2)?;
        for _ in 0..n {
            f.upvalues.push(Upvalue { in_stack: self.u8()?, index: self.u8()? });
        }
        let n = self.count(1)?;
        for _ in 0..n {
            f.protos.push(self.function(depth + 1)?);
        }
        let n = self.count(4)?;
        f.line_info = (0..n).map(|_| self.i32()).collect::<Result<_>>()?;
        let n = self.count(9)?;
        for _ in 0..n {
            f.loc_vars.push(LocVar { name: self.string()?, start_pc: self.i32()?, end_pc: self.i32()? });
        }
        let n = self.count(1)?;
        for _ in 0..n {
            f.upvalue_names.push(self.string()?);
        }
        Ok(f)
    }
}

fn write_count(w: &mut Vec<u8>, n: usize) {
    // `parse` never yields 2^31 items; saturate rather than wrap for a hand-built function.
    w.extend_from_slice(&i32::try_from(n).unwrap_or(i32::MAX).to_le_bytes());
}

fn write_string(w: &mut Vec<u8>, s: Option<&[u8]>) {
    match s {
        None => w.push(0),
        Some(s) => {
            let size = s.len() as u64 + 1;
            if size < 0xFF {
                w.push(size as u8);
            } else {
                w.push(0xFF);
                w.extend_from_slice(&size.to_le_bytes());
            }
            w.extend_from_slice(s);
        }
    }
}

fn write_function(w: &mut Vec<u8>, f: &Function) {
    write_string(w, f.source.as_deref());
    w.extend_from_slice(&f.line_defined.to_le_bytes());
    w.extend_from_slice(&f.last_line_defined.to_le_bytes());
    w.extend_from_slice(&[f.num_params, f.is_vararg, f.max_stack_size]);
    write_count(w, f.code.len());
    for c in &f.code {
        w.extend_from_slice(&c.to_le_bytes());
    }
    write_count(w, f.constants.len());
    for c in &f.constants {
        match c {
            Constant::Nil => w.push(0),
            Constant::Boolean(b) => w.extend_from_slice(&[1, u8::from(*b)]),
            Constant::Float(x) => {
                w.push(3);
                w.extend_from_slice(&x.to_bits().to_le_bytes());
            }
            Constant::Integer(n) => {
                w.push(19);
                w.extend_from_slice(&n.to_le_bytes());
            }
            Constant::ShortString(s) => {
                w.push(4);
                write_string(w, Some(s));
            }
            Constant::LongString(s) => {
                w.push(20);
                write_string(w, Some(s));
            }
        }
    }
    write_count(w, f.upvalues.len());
    for u in &f.upvalues {
        w.extend_from_slice(&[u.in_stack, u.index]);
    }
    write_count(w, f.protos.len());
    for p in &f.protos {
        write_function(w, p);
    }
    write_count(w, f.line_info.len());
    for l in &f.line_info {
        w.extend_from_slice(&l.to_le_bytes());
    }
    write_count(w, f.loc_vars.len());
    for v in &f.loc_vars {
        write_string(w, v.name.as_deref());
        w.extend_from_slice(&v.start_pc.to_le_bytes());
        w.extend_from_slice(&v.end_pc.to_le_bytes());
    }
    write_count(w, f.upvalue_names.len());
    for n in &f.upvalue_names {
        write_string(w, n.as_deref());
    }
}
```

- [ ] **Step 5: Run the tests until they pass.**

Run: `cargo fmt --all && cargo test -p fenji-lua`
Expected: 2 unit tests and 9 in `tests/chunk.rs` pass. The first build compiles Lua 5.3 from source through `mlua`'s `vendored` feature.

- [ ] **Step 6: Record Lua 5.3 via mlua in `docs/re/sources.md`.** Replace the line `No external sources used yet.` under "Used" with this table row:

```markdown
| [Lua 5.3](https://www.lua.org) (vendored by [mlua](https://github.com/mlua-rs/mlua) 0.12.2) | MIT | Yes: upstream Lua, built from source by the `lua-src` crate | Test-only: compiles our own Lua source into stock 5.3 bytecode fixtures at test time (M1 Task 4) | 2026-10-12 |
```

- [ ] **Step 7: Full gate, commit and merge.**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo deny check`
Expected: all pass. If `cargo deny` flags a licence in mlua's dependency tree that isn't on the allow list, stop. Adding a licence is a separate, deliberate `chore(deps)` change for the owner.

```bash
scripts/check-forbidden-files.sh --staged
git add Cargo.toml Cargo.lock crates/fenji-lua docs/re/sources.md
git commit -m "feat(lua): parse and write the client's Lua 5.3 chunks" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

Wait for CI, then squash-merge.

---

### Task 5: `fenji-lua`: disassembler

**Files:**
- Create: `crates/fenji-lua/src/disasm.rs`, `crates/fenji-lua/tests/disasm.rs`
- Modify: `crates/fenji-lua/src/lib.rs`

**Interfaces:**
- Consumes Task 4's `Chunk`, `Function`, `Constant`, `Instr`, `Op`, `Mode`, `is_k` and `k_index`.
- Produces `fenji_lua::disassemble(&Chunk) -> String`, a listing with:
  - one `function <path> (…)` header per prototype, with paths `0`, `0.0`, `0.1`, …;
  - one line per instruction, starting `  [<pc 1-based>] `;
  - RK constants as `-1-k` with the constant in a `;` note;
  - jumps noted `to [<pc>]`;
  - unknown codes as `OP?<code>`.

- [ ] **Step 1: Branch, then write the failing tests** in `crates/fenji-lua/tests/disasm.rs`:

```bash
git switch dev && git pull --ff-only && git switch -c feat/lua-<issue>-disasm
```

```rust
mod common;

use fenji_lua::{disassemble, parse, Dialect, Function, Instr, Op};

fn instructions(f: &Function) -> usize {
    f.code.len() + f.protos.iter().map(instructions).sum::<usize>()
}

#[test]
fn lists_every_function_and_instruction() {
    let chunk = parse(&common::luac(common::MODULE, true), Dialect::Stock53).unwrap();
    let text = disassemble(&chunk);
    assert_eq!(text.lines().filter(|l| l.starts_with("  [")).count(), instructions(&chunk.main));
    assert!(text.starts_with("function 0 ("));
    assert!(text.contains("function 0.0 ")); // M.pick
    assert!(text.contains("\"ipairs\""));
    assert!(text.contains("\"IsServer\""));
    assert!(text.contains("; to ["));
}

#[test]
fn client_opcodes_and_unknown_codes_are_shown() {
    let mut chunk = parse(&common::luac("print('hi')", true), Dialect::Stock53).unwrap();
    let first = Instr(chunk.main.code[0]);
    assert_eq!(first.op(), Some(Op::GetTabUp));
    chunk.main.code[0] = first.with_op(Op::TryTabUp).0;
    chunk.main.code.push(60); // in neither table
    let text = disassemble(&chunk);
    assert!(text.contains("TRYTABUP"));
    assert!(text.contains("\"print\""));
    assert!(text.contains("OP?60"));
}

#[test]
fn strings_are_escaped_but_utf8_is_kept() {
    let chunk = parse(&common::luac("return 'a\"b\\n', '\\xff', '中文'", true), Dialect::Stock53).unwrap();
    let text = disassemble(&chunk);
    assert!(text.contains(r#""a\"b\n""#));
    assert!(text.contains(r#""\xff""#));
    assert!(text.contains("\"中文\""));
}
```

Run: `cargo test -p fenji-lua --test disasm`
Expected: compile error (`fenji_lua::disassemble` not found).

- [ ] **Step 2: Implement `src/disasm.rs`.** Add `mod disasm;` and `pub use disasm::disassemble;` to `src/lib.rs`.

```rust
//! A text listing of a chunk, our ground truth for reading. One block per function, numbered by
//! its path in the prototype tree ("0", "0.1", …). One line per instruction, with its operands,
//! RK constants as `-1-k` (as `luac -l` prints them) plus the constant they name, and absolute
//! jump targets.
use std::fmt::Write as _;

use crate::{is_k, k_index, Chunk, Constant, Function, Instr, Mode, Op};

/// The listing of every function in `chunk`.
pub fn disassemble(chunk: &Chunk) -> String {
    let mut out = String::new();
    function(&mut out, &chunk.main, "0");
    out
}

fn function(out: &mut String, f: &Function, path: &str) {
    let _ = writeln!(
        out,
        "function {path} (params {}, vararg {}, stack {}, upvalues {}, constants {}, functions {}, instructions {})",
        f.num_params,
        f.is_vararg,
        f.max_stack_size,
        f.upvalues.len(),
        f.constants.len(),
        f.protos.len(),
        f.code.len()
    );
    for (pc, &word) in f.code.iter().enumerate() {
        let _ = writeln!(out, "  [{:>4}] {}", pc + 1, instruction(f, pc, Instr(word), path));
    }
    for (n, p) in f.protos.iter().enumerate() {
        function(out, p, &format!("{path}.{n}"));
    }
}

fn instruction(f: &Function, pc: usize, i: Instr, path: &str) -> String {
    let Some(op) = i.op() else {
        return format!("OP?{:<7} {:#010x}", i.code(), i.0);
    };
    let mut notes = Vec::new();
    let operands = match op.mode() {
        Mode::Abx => {
            match op {
                Op::LoadK => notes.push(constant(f, i.bx())),
                Op::Closure => notes.push(format!("function {path}.{}", i.bx())),
                _ => {}
            }
            format!("{} {}", i.a(), i.bx())
        }
        Mode::AsBx => {
            notes.push(format!("to [{}]", pc as i64 + 2 + i64::from(i.sbx())));
            format!("{} {}", i.a(), i.sbx())
        }
        Mode::Ax => i.ax().to_string(),
        Mode::Abc => {
            let (b_rk, c_rk) = op.rk_operands();
            let mut show = |x: u32, rk: bool| {
                if rk && is_k(x) {
                    notes.push(constant(f, k_index(x)));
                    (-1 - i64::from(k_index(x))).to_string()
                } else {
                    x.to_string()
                }
            };
            let (b, c) = (show(i.b(), b_rk), show(i.c(), c_rk));
            if matches!(op, Op::GetTabUp | Op::XGetTabUp | Op::TryTabUp | Op::TryLibUp) {
                notes.insert(0, format!("U{}", i.b()));
            } else if op == Op::SetTabUp {
                notes.insert(0, format!("U{}", i.a()));
            }
            format!("{} {b} {c}", i.a())
        }
    };
    let line = format!("{:<10} {operands}", op.name());
    if notes.is_empty() {
        line
    } else {
        format!("{line:<32}; {}", notes.join(" "))
    }
}

fn constant(f: &Function, k: u32) -> String {
    match f.constants.get(k as usize) {
        Some(Constant::Nil) => "nil".into(),
        Some(Constant::Boolean(b)) => b.to_string(),
        Some(Constant::Integer(n)) => n.to_string(),
        Some(Constant::Float(x)) => format!("{x:?}"),
        Some(Constant::ShortString(s) | Constant::LongString(s)) => quote(s),
        None => format!("K{k}?"),
    }
}

/// A string constant as a quoted literal. Valid UTF-8 stays as is (the client's UI text is
/// Chinese); quotes, backslashes and control characters are escaped; invalid bytes become \xNN.
fn quote(s: &[u8]) -> String {
    let mut out = String::from("\"");
    for chunk in s.utf8_chunks() {
        for ch in chunk.valid().chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => {
                    let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
                }
                c => out.push(c),
            }
        }
        for b in chunk.invalid() {
            let _ = write!(out, "\\x{b:02x}");
        }
    }
    out.push('"');
    out
}
```

- [ ] **Step 3: Run the tests until they pass, then run the gate.**

Run: `cargo fmt --all && cargo test -p fenji-lua`
Expected: all fenji-lua tests pass (2 unit, 9 chunk, 3 disasm).

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`
Expected: clean.

- [ ] **Step 4: Commit, PR and merge.**

```bash
git add crates/fenji-lua
git commit -m "feat(lua): disassemble the client's Lua chunks" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

Wait for CI, then squash-merge.

---

### Task 6: `luatool`: list, extract, disasm

**Files:**
- Modify: `Cargo.toml` (members), `Cargo.lock`
- Create:
  - `tools/luatool/Cargo.toml`
  - `tools/luatool/src/lib.rs`
  - `tools/luatool/src/main.rs`
  - `tools/luatool/src/client.rs`
  - `tools/luatool/src/output.rs`
  - `tools/luatool/src/commands.rs`
  - `tools/luatool/tests/cli.rs`
  - `tools/luatool/tests/pinned.rs`

**Interfaces:**
- Consumes:
  - Task 3: `PeImage`, `ExeFs`, `ExeEntry`, `BuiltinCodecs`, `sha256_hex`, `PINNED_EXE_SHA256`, `re_dir`, `test_util`
  - Tasks 4–5: `parse`, `write`, `Dialect`, `disassemble`
- Produces, for Tasks 7 and 11:
  - `fenji_luatool::client::{exe_path(Option<PathBuf>) -> Result<PathBuf>, load(&Path, any_build: bool) -> Result<PeImage>, decode(&ExeFs, &ExeEntry) -> Decoded, selected(&ExeFs, &str) -> impl Iterator<Item = &ExeEntry>, area(&str) -> String}`
  - `Decoded::{Lua(Vec<u8>, Dialect), Other(Vec<u8>), Failed(String)}`
  - `fenji_luatool::output::Out` (`new(&Path) -> Result<Out>`, `root() -> &Path`, `target(&mut self, path: &str, suffix: &str) -> Result<Option<PathBuf>>`, `write(&mut self, path, suffix, &[u8]) -> Result<Option<PathBuf>>`). `None` means the registry path was unsafe and nothing was written.
  - `fenji_luatool::commands::{Summary, list, extract, disasm}`
  - The binary `luatool`, with global `--exe PATH` and `--any-build`.

- [ ] **Step 1: Branch and manifest.**

```bash
git switch dev && git pull --ff-only && git switch -c feat/tools-<issue>-luatool
```

Add `"tools/luatool"` to the root `members`. Create `tools/luatool/Cargo.toml`:

```toml
[package]
name = "fenji-luatool"
description = "Lists, extracts, disassembles, normalises and decompiles the Lua scripts embedded in the pinned SoLO client; writes only under re/"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[[bin]]
name = "luatool"
path = "src/main.rs"

[dependencies]
anyhow.workspace = true
clap.workspace = true
fenji-data.workspace = true
fenji-lua.workspace = true

[dev-dependencies]
fenji-data = { workspace = true, features = ["test-util"] }
mlua.workspace = true
tempfile.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write the failing end-to-end tests.** `tools/luatool/tests/cli.rs` builds a synthetic exe from our own Lua source, with no game bytes:

```rust
//! luatool end to end, on a synthetic gujianol.exe built from our own Lua source (no game bytes).
use std::path::{Path, PathBuf};
use std::process::Command;

use fenji_data::test_util::{exe_with_files, lz4_blob};
use fenji_lua::Dialect;

fn client_chunk(src: &str) -> Vec<u8> {
    let lua = mlua::Lua::new();
    let stock = lua.load(src).into_function().unwrap().dump(true);
    fenji_lua::write(&fenji_lua::parse(&stock, Dialect::Stock53).unwrap(), Dialect::Client)
}

/// An entry truncated the way the image's large files are: the header claims 100 more compressed
/// bytes than the file has, and those bytes belong to the next object.
fn truncated_blob() -> Vec<u8> {
    let mut big = lz4_blob(&[7u8; 5000], 0);
    let comp = u32::from_le_bytes(big[32..36].try_into().unwrap()) + 100;
    big[32..36].copy_from_slice(&comp.to_le_bytes());
    let stored = u64::from_le_bytes(big[16..24].try_into().unwrap()) + 100;
    big[16..24].copy_from_slice(&stored.to_le_bytes());
    big.extend_from_slice(&[0x5A; 100]);
    big
}

/// A temporary checkout (`.git/`, `re/`) with a synthetic exe: a Lua chunk, an XML file, and a
/// truncated entry.
fn setup() -> (tempfile::TempDir, PathBuf) {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    std::fs::create_dir(repo.path().join("re")).unwrap();
    let (pe, _) = exe_with_files(&[
        ("addons/Login/Test.luac", lz4_blob(&client_chunk("if IsServer() then print('s') end"), 0)),
        ("addons/Login/Test.xml", lz4_blob(b"<UI/>", 0)),
        ("combat/big.luac", truncated_blob()),
    ]);
    let exe = repo.path().join("gujianol.exe");
    std::fs::write(&exe, pe).unwrap();
    (repo, exe)
}

fn luatool(repo: &Path, exe: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_luatool"));
    c.current_dir(repo).arg("--exe").arg(exe).arg("--any-build");
    c
}

#[test]
fn cli_lists_entries_with_status() {
    let (repo, exe) = setup();
    let out = luatool(repo.path(), &exe).arg("list").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{text}");
    assert!(lines[1].starts_with("addons/Login/Test.luac\t") && lines[1].ends_with("\tlua\tok"));
    assert!(lines[2].ends_with("\tfile\tok"));
    assert!(lines[3].starts_with("combat/big.luac\t") && lines[3].contains("undecodable"));
}

#[test]
fn cli_extracts_and_disassembles_under_re() {
    let (repo, exe) = setup();
    let ok = luatool(repo.path(), &exe).args(["extract", "--prefix", "addons/", "--out", "re/x"]).status().unwrap();
    assert!(ok.success());
    assert_eq!(std::fs::read(repo.path().join("re/x/addons/Login/Test.xml")).unwrap(), b"<UI/>");
    assert!(repo.path().join("re/x/addons/Login/Test.luac").is_file());
    let ok = luatool(repo.path(), &exe).args(["disasm", "--out", "re/d"]).status().unwrap();
    assert!(ok.success());
    let dis = std::fs::read_to_string(repo.path().join("re/d/addons/Login/Test.luac.dis")).unwrap();
    assert!(dis.contains("\"IsServer\""));
    let failures = std::fs::read_to_string(repo.path().join("re/d/disasm-failures.tsv")).unwrap();
    assert!(failures.contains("combat/big.luac"));
}

#[test]
fn cli_refuses_out_dir_outside_re() {
    let (repo, exe) = setup();
    let out = luatool(repo.path(), &exe).args(["extract", "--out", "docs"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not inside"));
    assert!(!repo.path().join("docs").exists());
}

#[test]
fn cli_refuses_unpinned_build() {
    let (repo, exe) = setup();
    let out = Command::new(env!("CARGO_BIN_EXE_luatool"))
        .current_dir(repo.path())
        .arg("--exe")
        .arg(&exe)
        .arg("list")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not the pinned build") && err.contains(fenji_data::PINNED_EXE_SHA256), "{err}");
}
```

`tools/luatool/tests/pinned.rs`, local only:

```rust
//! The real thing: the pinned client's embedded scripts. Local only; it needs the client copy.
use std::path::Path;

use fenji_luatool::client::{self, Decoded};

#[test]
#[ignore = "needs the pinned client: FENJI_CLIENT_PATH=~/fenji-client cargo test -p fenji-luatool --release -- --ignored"]
fn pinned_client_embedded_scripts() {
    let root = std::env::var_os("FENJI_CLIENT_PATH").expect("set FENJI_CLIENT_PATH");
    let image = client::load(&Path::new(&root).join("bin64").join("gujianol.exe"), false).unwrap();
    let fs = fenji_data::ExeFs::scan(&image).unwrap();
    // docs/re/m1-script-format.md §1. The registry count measured in Task 1 Step 2 goes here.
    assert_eq!(fs.entries().len(), 21_425);
    let (mut lua, mut other, mut undecodable) = (0u32, 0u32, 0u32);
    for e in fs.entries() {
        match client::decode(&fs, e) {
            Decoded::Lua(bytes, dialect) => {
                let chunk = fenji_lua::parse(&bytes, dialect).unwrap_or_else(|err| panic!("{}: {err}", e.path));
                assert!(!fenji_lua::disassemble(&chunk).is_empty());
                lua += 1;
            }
            Decoded::Other(_) => other += 1,
            Decoded::Failed(why) => {
                // m1-script-format §5: only entries cut at 16 KiB in the image fail.
                assert!(e.stored_size > 16_384, "{} ({} bytes) fails: {why}", e.path, e.stored_size);
                undecodable += 1;
            }
        }
    }
    eprintln!("{lua} Lua chunks parsed and disassembled, {other} other files, {undecodable} undecodable");
    assert!(lua > 19_000, "{lua}");
}
```

Run: `cargo test -p fenji-luatool`
Expected: compile errors (`fenji_luatool`, the `luatool` binary).

- [ ] **Step 3: Implement the library.**

`tools/luatool/src/lib.rs`:

```rust
//! The commands behind `luatool` (M1 spec §3), in a library so tests can drive them. Everything
//! it writes goes under re/ (ground rule 3).
pub mod client;
pub mod commands;
pub mod output;
```

`tools/luatool/src/client.rs`:

```rust
//! Opening the client: the exe's path, the pinned-build check, and decoding one entry.
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use fenji_data::{BuiltinCodecs, ExeEntry, ExeFs, PeImage};
use fenji_lua::Dialect;

/// `--exe`, else `$FENJI_CLIENT_PATH/bin64/gujianol.exe`.
pub fn exe_path(arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = arg {
        return Ok(p);
    }
    let root = std::env::var_os("FENJI_CLIENT_PATH")
        .context("pass --exe, or set FENJI_CLIENT_PATH to the pinned client copy (docs/re/pinned-build.md)")?;
    Ok(Path::new(&root).join("bin64").join("gujianol.exe"))
}

/// Reads and parses the exe. Refuses any build but the pinned one unless `any_build` is set.
pub fn load(path: &Path, any_build: bool) -> Result<PeImage> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let hash = fenji_data::sha256_hex(&bytes);
    if hash != fenji_data::PINNED_EXE_SHA256 && !any_build {
        bail!(
            "{} is not the pinned build: its SHA-256 is {hash}, the pinned one is {} \
             (docs/re/pinned-build.md); pass --any-build to read it anyway",
            path.display(),
            fenji_data::PINNED_EXE_SHA256
        );
    }
    Ok(PeImage::parse(bytes)?)
}

/// What decoding one registry entry gave.
pub enum Decoded {
    /// A Lua chunk, and its dialect (the client's, or stock).
    Lua(Vec<u8>, Dialect),
    /// Any other file: XML, TOC, config, C source …
    Other(Vec<u8>),
    /// The entry doesn't decode, like the large files cut at 16 KiB in the image.
    Failed(String),
}

pub fn decode(fs: &ExeFs, e: &ExeEntry) -> Decoded {
    match fs.read(e, &BuiltinCodecs) {
        Ok(bytes) => match Dialect::detect(&bytes) {
            Some(dialect) => Decoded::Lua(bytes, dialect),
            None => Decoded::Other(bytes),
        },
        Err(err) => Decoded::Failed(err.to_string()),
    }
}

/// The entries whose path starts with `prefix`.
pub fn selected<'f>(fs: &'f ExeFs<'_>, prefix: &'f str) -> impl Iterator<Item = &'f ExeEntry> {
    fs.entries().iter().filter(move |e| e.path.starts_with(prefix))
}

/// The area a path belongs to, for per-area counts: its first two directories.
pub fn area(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.len() {
        0 | 1 => "(top level)".into(),
        2 => parts[0].into(),
        _ => parts[..2].join("/"),
    }
}
```

`tools/luatool/src/output.rs`:

```rust
//! Writing under re/ only (ground rule 3).
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// An output directory inside the repository's re/.
pub struct Out {
    root: PathBuf,
    used: HashMap<PathBuf, u32>,
}

impl Out {
    /// `requested` is resolved from the current directory and must lie inside re/.
    pub fn new(requested: &Path) -> Result<Self> {
        let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
        let root = fenji_data::re_dir::resolve_out_dir(&repo, requested)?;
        Ok(Self { root, used: HashMap::new() })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the registry path `path`, plus `suffix`, goes. `None` if the path could leave the
    /// output directory. A path met twice gets `~2`, `~3` … so nothing is overwritten.
    pub fn target(&mut self, path: &str, suffix: &str) -> Result<Option<PathBuf>> {
        let Some(rel) = fenji_data::re_dir::safe_relative(path) else {
            return Ok(None);
        };
        let mut name = rel.into_os_string();
        name.push(suffix);
        let mut target = self.root.join(name);
        let seen = self.used.entry(target.clone()).or_insert(0);
        *seen += 1;
        if *seen > 1 {
            let mut s = target.into_os_string();
            s.push(format!("~{seen}"));
            target = s.into();
        }
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(Some(target))
    }

    /// Writes `bytes` to [`Out::target`]; returns where, or `None` for an unsafe path.
    pub fn write(&mut self, path: &str, suffix: &str, bytes: &[u8]) -> Result<Option<PathBuf>> {
        let Some(target) = self.target(path, suffix)? else {
            return Ok(None);
        };
        std::fs::write(&target, bytes).with_context(|| format!("writing {}", target.display()))?;
        Ok(Some(target))
    }
}
```

`tools/luatool/src/commands.rs`:

```rust
//! luatool's commands. Each returns a [`Summary`]; per-entry failures are counted, never fatal.
use std::io::Write;

use anyhow::Result;
use fenji_data::ExeFs;

use crate::client::{decode, selected, Decoded};
use crate::output::Out;

/// Counts for one command: printed at the end, with the failures written to
/// `<command>-failures.tsv`.
#[derive(Debug, Default)]
pub struct Summary {
    pub lua: u64,
    pub other: u64,
    pub undecodable: u64,
    pub bad_chunks: u64,
    pub failures: Vec<(String, String)>,
}

impl Summary {
    pub fn fail(&mut self, path: &str, why: impl std::fmt::Display) {
        self.failures.push((path.to_string(), why.to_string()));
    }

    /// One line for humans.
    pub fn line(&self, command: &str) -> String {
        format!(
            "{command}: {} Lua chunks, {} other files, {} undecodable entries, {} chunks refused or unparsable, {} failures listed",
            self.lua,
            self.other,
            self.undecodable,
            self.bad_chunks,
            self.failures.len()
        )
    }

    /// Writes the failures into the output directory.
    pub fn write_failures(&self, out: &Out, command: &str) -> Result<()> {
        let mut text = String::from("path\treason\n");
        for (path, why) in &self.failures {
            text.push_str(&format!("{path}\t{}\n", why.replace(['\t', '\n'], " ")));
        }
        std::fs::write(out.root().join(format!("{command}-failures.tsv")), text)?;
        Ok(())
    }
}

/// `list`: one TSV line per selected entry: path, stored size, raw size, kind, status.
pub fn list(fs: &ExeFs, prefix: &str, w: &mut impl Write) -> Result<Summary> {
    let mut s = Summary::default();
    writeln!(w, "path\tstored\traw\tkind\tstatus")?;
    for e in selected(fs, prefix) {
        let raw = fs.header(e).map_or_else(|_| "?".to_string(), |h| h.raw_size.to_string());
        let (kind, status) = match decode(fs, e) {
            Decoded::Lua(bytes, dialect) => {
                s.lua += 1;
                match fenji_lua::parse(&bytes, dialect) {
                    Ok(_) => ("lua", "ok".to_string()),
                    Err(err) => {
                        s.bad_chunks += 1;
                        s.fail(&e.path, &err);
                        ("lua", format!("bad chunk: {err}"))
                    }
                }
            }
            Decoded::Other(_) => {
                s.other += 1;
                ("file", "ok".to_string())
            }
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&e.path, &why);
                ("?", format!("undecodable: {why}"))
            }
        };
        writeln!(w, "{}\t{}\t{raw}\t{kind}\t{status}", e.path, e.stored_size)?;
    }
    Ok(s)
}

/// `extract`: every decodable entry's content, exactly as the client stores it, under `out`.
pub fn extract(fs: &ExeFs, prefix: &str, out: &mut Out) -> Result<Summary> {
    let mut s = Summary::default();
    for e in selected(fs, prefix) {
        let bytes = match decode(fs, e) {
            Decoded::Lua(bytes, _) => {
                s.lua += 1;
                bytes
            }
            Decoded::Other(bytes) => {
                s.other += 1;
                bytes
            }
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&e.path, why);
                continue;
            }
        };
        if out.write(&e.path, "", &bytes)?.is_none() {
            s.fail(&e.path, "unsafe path in the registry; not written");
        }
    }
    Ok(s)
}

/// `disasm`: a listing, `<path>.dis`, of every Lua chunk that parses.
pub fn disasm(fs: &ExeFs, prefix: &str, out: &mut Out) -> Result<Summary> {
    let mut s = Summary::default();
    for e in selected(fs, prefix) {
        match decode(fs, e) {
            Decoded::Lua(bytes, dialect) => {
                s.lua += 1;
                match fenji_lua::parse(&bytes, dialect) {
                    Ok(chunk) => {
                        if out.write(&e.path, ".dis", fenji_lua::disassemble(&chunk).as_bytes())?.is_none() {
                            s.fail(&e.path, "unsafe path in the registry; not written");
                        }
                    }
                    Err(err) => {
                        s.bad_chunks += 1;
                        s.fail(&e.path, err);
                    }
                }
            }
            Decoded::Other(_) => s.other += 1,
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&e.path, why);
            }
        }
    }
    Ok(s)
}
```

- [ ] **Step 4: Implement the binary**, `tools/luatool/src/main.rs`:

```rust
//! luatool: the Lua scripts embedded in the pinned SoLO client (M1 spec §3). Writes only under re/.
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use fenji_luatool::{client, commands, output::Out};

/// The Lua scripts embedded in the pinned SoLO client. Writes only under re/.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The client's gujianol.exe (default: $FENJI_CLIENT_PATH/bin64/gujianol.exe)
    #[arg(long, global = true)]
    exe: Option<PathBuf>,
    /// Read an exe that isn't the pinned build (docs/re/pinned-build.md)
    #[arg(long, global = true)]
    any_build: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the embedded files as TSV on stdout: path, stored size, raw size, kind, status
    List {
        /// Only paths starting with this
        #[arg(long, default_value = "")]
        prefix: String,
    },
    /// Write the decoded files, as the client stores them, under OUT (inside re/)
    Extract {
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write a disassembly (<path>.dis) of each Lua chunk under OUT (inside re/)
    Disasm {
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Check the output directory before the 50 MB exe is read.
    let mut out = match &cli.command {
        Command::List { .. } => None,
        Command::Extract { out, .. } | Command::Disasm { out, .. } => Some(Out::new(out)?),
    };
    let image = client::load(&client::exe_path(cli.exe.clone())?, cli.any_build)?;
    let fs = fenji_data::ExeFs::scan(&image)?;
    let (name, summary) = match (&cli.command, out.as_mut()) {
        (Command::List { prefix }, _) => ("list", commands::list(&fs, prefix, &mut std::io::stdout().lock())?),
        (Command::Extract { prefix, .. }, Some(o)) => ("extract", commands::extract(&fs, prefix, o)?),
        (Command::Disasm { prefix, .. }, Some(o)) => ("disasm", commands::disasm(&fs, prefix, o)?),
        _ => unreachable!("every command that writes has an output directory"),
    };
    if let Some(o) = &out {
        summary.write_failures(o, name)?;
        eprintln!("output: {}", o.root().display());
    }
    eprintln!("{}", summary.line(name));
    Ok(())
}
```

- [ ] **Step 5: Run the tests until they pass.**

Run: `cargo fmt --all && cargo test -p fenji-luatool`
Expected: the 4 `cli.rs` tests pass; `pinned.rs` reports 1 ignored.

- [ ] **Step 6: Run against the pinned client** (local). Record the numbers.

```bash
FENJI_CLIENT_PATH=~/fenji-client cargo test -p fenji-luatool --release -- --ignored --nocapture
cargo build --release -p fenji-luatool
target/release/luatool list > re/m1/list.tsv
target/release/luatool extract --out re/m1/extracted
target/release/luatool disasm --out re/m1/disasm
```

Expected:
- The ignored test passes, printing Lua, other and undecodable counts.
- `re/m1/disasm` holds one `.dis` per decodable chunk, and `re/m1/disasm/disasm-failures.tsv` lists only entries with stored size above 16,384.

If the counts differ from `docs/re/m1-script-format.md`, correct that doc in this PR, with the measured value and `luatool` as the evidence. Nothing under `re/` is committed.

- [ ] **Step 7: Gate, commit and merge.**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo deny check`

```bash
scripts/check-forbidden-files.sh --staged
git add Cargo.toml Cargo.lock tools/luatool docs/re/m1-script-format.md
git commit -m "feat(tools): add luatool to list, extract and disassemble the embedded scripts" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

Wait for CI, then squash-merge.

---

### Task 7: Normaliser, `luatool normalize` and `decompile`, unluac pin

**Files:**
- Create:
  - `crates/fenji-lua/src/normalize.rs`
  - `crates/fenji-lua/tests/normalize.rs`
  - `tools/luatool/src/decompile.rs`
  - `tools/luatool/tests/decompile.rs`
- Modify:
  - `crates/fenji-lua/src/lib.rs`
  - `tools/luatool/src/lib.rs`
  - `tools/luatool/src/main.rs`
  - `tools/luatool/src/commands.rs`
  - `scripts/versions.env`
  - `scripts/bootstrap.sh`
  - `CLAUDE.md` (pinned table)
  - `docs/development.md`
  - `docs/re/sources.md`

**Interfaces:**
- Consumes:
  - Task 1 §4's normalisation table, which `stock_equivalent` must equal;
  - Tasks 4–6.
- Produces:
  - `fenji_lua::normalize(&Chunk) -> Result<(Chunk, Report)>` and `fenji_lua::stock_equivalent(Op) -> Option<Op>`
  - `fenji_lua::Report { rewritten: BTreeMap<&'static str, u64> }`
  - `fenji_luatool::decompile::{Unluac, Outcome}`
  - `luatool normalize --out DIR` and `luatool decompile --out DIR [--jar J] [--java J] [--jobs N] [--timeout S]`
  - `UNLUAC_VERSION` and `UNLUAC_SHA256` in `scripts/versions.env`

- [ ] **Step 1: Branch, and check the mapping against Task 1.**

```bash
git switch dev && git pull --ff-only && git switch -c feat/lua-<issue>-normalize
```

Open `docs/re/m1-script-format.md` §4. The code below maps `TRYTABUP` and `TRYLIBUP` to `GETTABUP`, `TRYSELF` to `SELF`, and leaves `NOP`, `XGETTABUP`, `XGETSELF` and `XSELF` unmapped. If §4 says otherwise, change `stock_equivalent`'s match arms to equal §4 before writing anything else, and adjust `clientify` in the test to swap only opcodes §4 maps.

- [ ] **Step 2: Write the failing normaliser tests**, `crates/fenji-lua/tests/normalize.rs`:

```rust
mod common;

use fenji_lua::{normalize, parse, write, Dialect, Error, Function, Instr, Op};

/// Swaps in the client's opcodes the way its compiler uses them: GETTABUP → TRYTABUP and
/// SELF → TRYSELF. Returns how many it swapped.
fn clientify(f: &mut Function) -> u64 {
    let mut n = 0;
    for word in &mut f.code {
        let i = Instr(*word);
        let to = match i.op() {
            Some(Op::GetTabUp) => Op::TryTabUp,
            Some(Op::SelfOp) => Op::TrySelf,
            _ => continue,
        };
        *word = i.with_op(to).0;
        n += 1;
    }
    n + f.protos.iter_mut().map(clientify).sum::<u64>()
}

#[test]
fn normalising_restores_the_stock_bytes() {
    let stock = common::luac(common::MODULE, true);
    let mut chunk = parse(&stock, Dialect::Stock53).unwrap();
    let swapped = clientify(&mut chunk.main);
    assert!(swapped > 0);
    let client = parse(&write(&chunk, Dialect::Client), Dialect::Client).unwrap();
    let (normal, report) = normalize(&client).unwrap();
    assert_eq!(write(&normal, Dialect::Stock53), stock);
    assert_eq!(report.rewritten.values().sum::<u64>(), swapped);
    assert!(report.rewritten.contains_key("TRYTABUP") && report.rewritten.contains_key("TRYSELF"));
}

#[test]
fn normalising_keeps_the_shape() {
    fn same_shape(a: &Function, b: &Function) {
        assert_eq!(a.code.len(), b.code.len());
        assert_eq!(a.constants, b.constants);
        assert_eq!(a.upvalues, b.upvalues);
        for (x, y) in a.code.iter().zip(&b.code) {
            let (x, y) = (Instr(*x), Instr(*y));
            assert_eq!(x.0 & !0x3F, y.0 & !0x3F, "operands never change");
            if x.op().unwrap().is_stock() {
                assert_eq!(x, y, "stock instructions never change");
            }
        }
        assert_eq!(a.protos.len(), b.protos.len());
        a.protos.iter().zip(&b.protos).for_each(|(p, q)| same_shape(p, q));
    }
    let mut chunk = parse(&common::luac(common::MODULE, true), Dialect::Stock53).unwrap();
    clientify(&mut chunk.main);
    let (normal, _) = normalize(&chunk).unwrap();
    same_shape(&chunk.main, &normal.main);
}

#[test]
fn unmapped_opcode_refuses_the_chunk() {
    let mut chunk = parse(&common::luac("print('hi')", true), Dialect::Stock53).unwrap();
    chunk.main.code[0] = Instr(chunk.main.code[0]).with_op(Op::Nop).0;
    assert_eq!(normalize(&chunk).unwrap_err(), Error::Unmapped { op: "NOP" });
}

#[test]
fn unknown_code_refuses_the_chunk() {
    let mut chunk = parse(&common::luac("print('hi')", true), Dialect::Stock53).unwrap();
    chunk.main.code.insert(0, 60);
    assert_eq!(normalize(&chunk).unwrap_err(), Error::UnknownOpcode { code: 60 });
}
```

Run: `cargo test -p fenji-lua --test normalize`
Expected: compile error (`fenji_lua::normalize` not found).

- [ ] **Step 3: Implement `crates/fenji-lua/src/normalize.rs`.** Add `mod normalize;` and `pub use normalize::{normalize, stock_equivalent, Report};` to `lib.rs`.

```rust
//! The client's dialect → stock Lua 5.3, for the external decompiler (M1 spec §3). Each
//! instruction stays exactly one instruction, so jump offsets, constants and nested functions
//! never move. Only client-only opcodes change, each to its stock equivalent from the VM reading
//! in docs/re/m1-script-format.md §4. The disassembly stays the ground truth.
use std::collections::BTreeMap;

use crate::{Chunk, Error, Function, Instr, Op, Result};

/// The stock opcode a client-only opcode becomes; stock opcodes map to themselves. `None` means no
/// equivalent is established yet, and chunks using that opcode are refused rather than
/// decompiled wrongly.
pub fn stock_equivalent(op: Op) -> Option<Op> {
    match op {
        Op::TryTabUp | Op::TryLibUp => Some(Op::GetTabUp),
        Op::TrySelf => Some(Op::SelfOp),
        Op::Nop | Op::XGetTabUp | Op::XGetSelf | Op::XSelf => None,
        stock => Some(stock),
    }
}

/// What a normalisation changed: client opcode name → instructions rewritten.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub rewritten: BTreeMap<&'static str, u64>,
}

/// A stock Lua 5.3 copy of `chunk` (write it with `Dialect::Stock53`), and what changed.
pub fn normalize(chunk: &Chunk) -> Result<(Chunk, Report)> {
    let mut out = chunk.clone();
    let mut report = Report::default();
    rewrite(&mut out.main, &mut report)?;
    Ok((out, report))
}

fn rewrite(f: &mut Function, report: &mut Report) -> Result<()> {
    for word in &mut f.code {
        let i = Instr(*word);
        let op = i.op().ok_or(Error::UnknownOpcode { code: i.code() })?;
        if op.is_stock() {
            continue;
        }
        let stock = stock_equivalent(op).ok_or(Error::Unmapped { op: op.name() })?;
        *word = i.with_op(stock).0;
        *report.rewritten.entry(op.name()).or_default() += 1;
    }
    f.protos.iter_mut().try_for_each(|p| rewrite(p, report))
}
```

Run: `cargo test -p fenji-lua`
Expected: all pass, including the 4 normalize tests.

- [ ] **Step 4: Write the failing runner tests**, `tools/luatool/tests/decompile.rs`. They use a stand-in for `java`, so CI needs neither Java nor the jar:

```rust
#![cfg(unix)]
//! The unluac runner, with a stand-in for `java -jar unluac.jar FILE`: it echoes, fails or hangs
//! depending on FILE's name.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fenji_luatool::decompile::{Outcome, Unluac};

fn fake_java(dir: &Path) -> PathBuf {
    let p = dir.join("fake-java");
    std::fs::write(
        &p,
        "#!/bin/sh\ncase \"$3\" in *slow*) sleep 10 ;; *fail*) echo boom >&2; exit 1 ;; esac\necho \"-- decompiled $3\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn unluac(dir: &Path, timeout: Duration) -> Unluac {
    let jar = dir.join("unluac.jar");
    std::fs::write(&jar, b"").unwrap();
    Unluac::locate(Some(fake_java(dir)), Some(jar), dir, timeout).unwrap()
}

#[test]
fn outcomes_and_outputs() {
    let t = tempfile::tempdir().unwrap();
    let u = unluac(t.path(), Duration::from_secs(5));
    let jobs: Vec<(PathBuf, PathBuf)> = ["ok", "fail", "ok2"]
        .iter()
        .map(|n| (t.path().join(format!("{n}.luac")), t.path().join(format!("{n}.lua"))))
        .collect();
    assert_eq!(u.run_all(&jobs, 2), [Outcome::Ok, Outcome::Failed, Outcome::Ok]);
    assert!(std::fs::read_to_string(t.path().join("ok.lua")).unwrap().starts_with("-- decompiled"));
    assert!(!t.path().join("ok.lua.err").exists());
    assert_eq!(std::fs::read_to_string(t.path().join("fail.lua.err")).unwrap().trim(), "boom");
}

#[test]
fn a_hanging_decompile_is_stopped() {
    let t = tempfile::tempdir().unwrap();
    let u = unluac(t.path(), Duration::from_millis(500));
    let started = Instant::now();
    let outcome = u.run(&t.path().join("slow.luac"), &t.path().join("slow.lua")).unwrap();
    assert_eq!(outcome, Outcome::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_missing_jar_is_reported() {
    let t = tempfile::tempdir().unwrap();
    let err = Unluac::locate(Some("java".into()), Some(t.path().join("nope.jar")), t.path(), Duration::from_secs(1))
        .err()
        .unwrap();
    assert!(err.to_string().contains("unluac jar not found"));
}

#[test]
#[ignore = "needs Java and the pinned unluac jar (scripts/bootstrap.sh)"]
fn real_unluac_decompiles_a_stripped_chunk() {
    let t = tempfile::tempdir().unwrap();
    let lua = mlua::Lua::new();
    let chunk = lua
        .load("local s = { host = '127.0.0.1' } return s")
        .into_function()
        .unwrap()
        .dump(true);
    let input = t.path().join("t.luac");
    std::fs::write(&input, chunk).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let u = Unluac::locate(None, None, &repo, Duration::from_secs(60)).unwrap();
    assert_eq!(u.run(&input, &t.path().join("t.lua")).unwrap(), Outcome::Ok);
    assert!(std::fs::read_to_string(t.path().join("t.lua")).unwrap().contains("127.0.0.1"));
}
```

Run: `cargo test -p fenji-luatool --test decompile`
Expected: compile error (`fenji_luatool::decompile` not found).

- [ ] **Step 5: Implement `tools/luatool/src/decompile.rs`.** Add `pub mod decompile;` to `src/lib.rs`.

```rust
//! Running unluac (MIT; an external tool, never linked; docs/re/sources.md) on normalised chunks.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

/// How one decompilation went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Failed,
    TimedOut,
}

/// The java binary, the pinned unluac jar, and a per-chunk time limit.
pub struct Unluac {
    java: PathBuf,
    jar: PathBuf,
    timeout: Duration,
}

impl Unluac {
    /// `java`: `--java`, else `$JAVA_HOME/bin/java`, else `java` on PATH. `jar`: `--jar`, else the
    /// pinned jar that scripts/bootstrap.sh installs (`UNLUAC_VERSION` in scripts/versions.env).
    pub fn locate(java: Option<PathBuf>, jar: Option<PathBuf>, repo: &Path, timeout: Duration) -> Result<Self> {
        let java = java
            .or_else(|| std::env::var_os("JAVA_HOME").map(|h| Path::new(&h).join("bin").join("java")))
            .unwrap_or_else(|| PathBuf::from("java"));
        let jar = match jar {
            Some(j) => j,
            None => pinned_jar(repo)?,
        };
        if !jar.is_file() {
            bail!("unluac jar not found at {} (run scripts/bootstrap.sh, or pass --jar)", jar.display());
        }
        Ok(Self { java, jar, timeout })
    }

    /// Decompiles `input` into `output`. On failure, unluac's stderr is kept in `<output>.err`.
    pub fn run(&self, input: &Path, output: &Path) -> Result<Outcome> {
        let err_path = with_suffix(output, ".err");
        let mut child = Command::new(&self.java)
            .arg("-jar")
            .arg(&self.jar)
            .arg(input)
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(output)?)
            .stderr(std::fs::File::create(&err_path)?)
            .spawn()
            .with_context(|| format!("starting {}", self.java.display()))?;
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                if status.success() {
                    std::fs::remove_file(&err_path).ok();
                    return Ok(Outcome::Ok);
                }
                return Ok(Outcome::Failed);
            }
            if start.elapsed() > self.timeout {
                child.kill().ok();
                child.wait().ok();
                return Ok(Outcome::TimedOut);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Runs every (input, output) pair on `workers` threads. Outcomes come back in input order.
    pub fn run_all(&self, jobs: &[(PathBuf, PathBuf)], workers: usize) -> Vec<Outcome> {
        let next = AtomicUsize::new(0);
        let results = Mutex::new(vec![Outcome::Failed; jobs.len()]);
        std::thread::scope(|s| {
            for _ in 0..workers.max(1) {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((input, output)) = jobs.get(i) else { break };
                    let outcome = self.run(input, output).unwrap_or(Outcome::Failed);
                    results.lock().expect("no worker panics while holding the lock")[i] = outcome;
                });
            }
        });
        results.into_inner().expect("all workers finished")
    }
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    s.into()
}

/// `~/.local/share/fenji/tools/unluac/unluac_<UNLUAC_VERSION>.jar`.
fn pinned_jar(repo: &Path) -> Result<PathBuf> {
    let env = std::fs::read_to_string(repo.join("scripts").join("versions.env")).context("reading scripts/versions.env")?;
    let version = env
        .lines()
        .find_map(|l| l.strip_prefix("UNLUAC_VERSION="))
        .map(|v| v.split('#').next().unwrap_or_default().trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
        .context("UNLUAC_VERSION is missing from scripts/versions.env")?;
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(Path::new(&home).join(format!(".local/share/fenji/tools/unluac/unluac_{version}.jar")))
}
```

- [ ] **Step 6: Add the `normalize` and `decompile` commands.** Append to `tools/luatool/src/commands.rs`:

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;

use fenji_lua::Dialect;

use crate::client::area;
use crate::decompile::{Outcome, Unluac};

/// A Lua chunk normalised to stock 5.3 bytes.
fn stock_bytes(bytes: &[u8], dialect: Dialect) -> fenji_lua::Result<Vec<u8>> {
    let chunk = fenji_lua::parse(bytes, dialect)?;
    let (normal, _) = fenji_lua::normalize(&chunk)?;
    Ok(fenji_lua::write(&normal, Dialect::Stock53))
}

/// `normalize`: each Lua chunk rewritten to stock Lua 5.3 (still `<path>`, a .luac) under `out`.
pub fn normalize(fs: &ExeFs, prefix: &str, out: &mut Out) -> Result<Summary> {
    let mut s = Summary::default();
    for e in selected(fs, prefix) {
        match decode(fs, e) {
            Decoded::Lua(bytes, dialect) => {
                s.lua += 1;
                match stock_bytes(&bytes, dialect) {
                    Ok(stock) => {
                        if out.write(&e.path, "", &stock)?.is_none() {
                            s.fail(&e.path, "unsafe path in the registry; not written");
                        }
                    }
                    Err(err) => {
                        s.bad_chunks += 1;
                        s.fail(&e.path, err);
                    }
                }
            }
            Decoded::Other(_) => s.other += 1,
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&e.path, why);
            }
        }
    }
    Ok(s)
}

/// Per-area decompile counts: chunks, decompiled, refused by the normaliser, unluac failed,
/// timed out.
type AreaCounts = [u64; 5];

/// `decompile`: normalise each chunk into `out/.stock/`, decompile it with unluac into
/// `out/<path without .luac>.lua`, and write per-area counts to `out/decompile-summary.tsv`.
pub fn decompile(fs: &ExeFs, prefix: &str, out: &mut Out, unluac: &Unluac, workers: usize) -> Result<Summary> {
    let mut s = Summary::default();
    let mut areas: BTreeMap<String, AreaCounts> = BTreeMap::new();
    let mut jobs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut job_info: Vec<(String, String)> = Vec::new(); // (area, path) per job
    for e in selected(fs, prefix) {
        let (bytes, dialect) = match decode(fs, e) {
            Decoded::Lua(bytes, dialect) => (bytes, dialect),
            Decoded::Other(_) => {
                s.other += 1;
                continue;
            }
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&e.path, why);
                continue;
            }
        };
        s.lua += 1;
        let a = area(&e.path);
        areas.entry(a.clone()).or_default()[0] += 1;
        let stock = match stock_bytes(&bytes, dialect) {
            Ok(stock) => stock,
            Err(err) => {
                areas.entry(a).or_default()[2] += 1;
                s.bad_chunks += 1;
                s.fail(&e.path, err);
                continue;
            }
        };
        let name = e.path.strip_suffix(".luac").map_or_else(|| format!("{}.lua", e.path), |p| format!("{p}.lua"));
        let (Some(input), Some(output)) = (out.write(&format!(".stock/{}", e.path), "", &stock)?, out.target(&name, "")?) else {
            s.fail(&e.path, "unsafe path in the registry; not written");
            continue;
        };
        jobs.push((input, output));
        job_info.push((a, e.path.clone()));
    }
    for ((a, path), outcome) in job_info.iter().zip(unluac.run_all(&jobs, workers)) {
        let counts = areas.entry(a.clone()).or_default();
        match outcome {
            Outcome::Ok => counts[1] += 1,
            Outcome::Failed => {
                counts[3] += 1;
                s.fail(path, "unluac failed (see the .lua.err next to the output)");
            }
            Outcome::TimedOut => {
                counts[4] += 1;
                s.fail(path, "unluac timed out");
            }
        }
    }
    let mut tsv = String::from("area\tchunks\tdecompiled\tnormaliser_refused\tunluac_failed\ttimed_out\n");
    for (a, [n, ok, refused, failed, timeout]) in &areas {
        tsv.push_str(&format!("{a}\t{n}\t{ok}\t{refused}\t{failed}\t{timeout}\n"));
    }
    std::fs::write(out.root().join("decompile-summary.tsv"), tsv)?;
    Ok(s)
}
```

In `tools/luatool/src/main.rs`, add `use std::time::Duration;` and `use fenji_luatool::decompile::Unluac;`. Add two variants to `Command`:

```rust
    /// Write each Lua chunk normalised to stock Lua 5.3 under OUT (inside re/)
    Normalize {
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Decompile each Lua chunk with unluac into OUT (inside re/)
    Decompile {
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        out: PathBuf,
        /// The unluac jar (default: the pinned one that scripts/bootstrap.sh installs)
        #[arg(long)]
        jar: Option<PathBuf>,
        /// The java binary (default: $JAVA_HOME/bin/java, else java on PATH)
        #[arg(long)]
        java: Option<PathBuf>,
        /// Parallel unluac processes (default: the number of CPUs)
        #[arg(long)]
        jobs: Option<usize>,
        /// Seconds per chunk before unluac is stopped
        #[arg(long, default_value_t = 120)]
        timeout: u64,
    },
```

Extend the two matches in `main`. In the `out` match:

```rust
        Command::Extract { out, .. }
        | Command::Disasm { out, .. }
        | Command::Normalize { out, .. }
        | Command::Decompile { out, .. } => Some(Out::new(out)?),
```

Then add these arms before the `unreachable!` arm:

```rust
        (Command::Normalize { prefix, .. }, Some(o)) => ("normalize", commands::normalize(&fs, prefix, o)?),
        (Command::Decompile { prefix, jar, java, jobs, timeout, .. }, Some(o)) => {
            let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
            let unluac = Unluac::locate(java.clone(), jar.clone(), &repo, Duration::from_secs(*timeout))?;
            let workers = jobs.unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()));
            ("decompile", commands::decompile(&fs, prefix, o, &unluac, workers)?)
        }
```

Run: `cargo fmt --all && cargo test -p fenji-luatool`
Expected: `cli.rs` 4 pass; `decompile.rs` 3 pass with 1 ignored; `pinned.rs` 1 ignored.

- [ ] **Step 7: Pin unluac.** `scripts/versions.env`, after the Ghidra lines:

```bash
# Lua decompiler: an external tool whose output stays in re/. MIT licence (docs/re/sources.md).
UNLUAC_VERSION="2025_12_23"
# Upstream publishes no checksum; computed from the official SourceForge asset on 2026-10-12.
UNLUAC_SHA256="98be0fa84ac73ca66dce2842a2e4512226f4c611b6500dc96415571fc5538fcc"
```

`scripts/bootstrap.sh`:
- next to the other `*_DIR` definitions near the top, add `UNLUAC_JAR="$TOOLS_DIR/unluac/unluac_${UNLUAC_VERSION}.jar"`;
- in `check_all()`, after the Ghidra report, add `report unluac "$UNLUAC_VERSION" "$([ -f "$UNLUAC_JAR" ] && echo "$UNLUAC_VERSION")"`;
- in section 5, after the DepotDownloader block, add:

```bash
if [ ! -f "$UNLUAC_JAR" ]; then
  log "unluac $UNLUAC_VERSION (SHA-256 verified)"
  mkdir -p "$(dirname "$UNLUAC_JAR")"
  download_verified \
    "https://downloads.sourceforge.net/project/unluac/Unstable/unluac_${UNLUAC_VERSION}.jar" \
    "$UNLUAC_SHA256" "$UNLUAC_JAR"
fi
```

Add a row to `CLAUDE.md`'s pinned table, after Ghidra, in the same commit:

```markdown
| unluac | 2025_12_23 | Lua 5.0–5.4 decompiler jar (MIT), SHA-256 pinned, `~/.local/share/fenji/tools/unluac/`; external tool, output only in `re/` |
```

In `docs/development.md`, append to the bullet that lists what lands in `~/.local/bin` and the tools directory: `; the unluac jar lands in ~/.local/share/fenji/tools/unluac/ (used by luatool decompile)`.

Add a `docs/re/sources.md` "Used" row:

```markdown
| [unluac](https://sourceforge.net/projects/unluac/) `unluac_2025_12_23.jar` | MIT (project page) | Yes: a general Lua 5.0–5.4 decompiler by its author; nothing SoLO-specific | External tool: decompiles our normalised copies of the client's own scripts; output only in `re/` (M1 Task 7) | 2026-10-12 |
```

Run: `scripts/bootstrap.sh --check | grep unluac`, then `scripts/bootstrap.sh`, then `scripts/bootstrap.sh --check | grep unluac`.
Expected: `unluac` is missing before and `2025_12_23` after. `download_verified` fails loudly if the checksum differs.

- [ ] **Step 8: Run against the pinned client** (local):

```bash
cargo build --release -p fenji-luatool
FENJI_CLIENT_PATH=~/fenji-client target/release/luatool decompile --prefix addons/Login/ --out re/m1/decompiled
FENJI_CLIENT_PATH=~/fenji-client target/release/luatool decompile --out re/m1/decompiled-all
cargo test -p fenji-luatool -- --ignored real_unluac
```

Expected:
- `re/m1/decompiled/addons/Login/Login.lua` exists, and `decompile-summary.tsv` shows its area decompiled.
- The full run's summary gives per-area success rates; this is Done-when 1's report.
- The ignored real-unluac test passes.

Add the per-area table (numbers only) to `docs/re/m1-script-format.md` §4 as "Decompile results", with `re/m1/decompiled-all/decompile-summary.tsv` as evidence.

- [ ] **Step 9: Gate, commit and merge.**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo deny check && scripts/check-forbidden-files.sh`

```bash
git add crates/fenji-lua tools/luatool scripts/versions.env scripts/bootstrap.sh CLAUDE.md docs/development.md docs/re/sources.md docs/re/m1-script-format.md
git commit -m "feat(lua): normalise client chunks to stock Lua 5.3 and decompile them with unluac" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

Wait for CI, then squash-merge.

---

### Task 8: Login-stage hunt: what gives the client its server address

**Files:**
- Create: `docs/protocol/login.md`
- Modify: `docs/README.md` (one row)
- Local only: `re/m1/login/`

**Interfaces:**
- Consumes:
  - Task 6's `re/m1/disasm/` and Task 7's `re/m1/decompiled-all/`;
  - spike-0 8.15–8.17 (the login scene and the server-list failure), 8.32 (the `servername` probe at the install root) and 8.37 (the `--server-name`, `--server_locale` and `--net` flags);
  - spike-0's Ghidra note on the Lua binding `net_client_create` at 0x140222970, which reads two strings and an integer.
- Produces, for Task 10, `docs/protocol/login.md` §3: **the exact input** (file name and format, flags, or hosts names) that should point the client at `127.0.0.1:<port>`, and the port or ports it should try.

This is RE work. Only a session or agent that has never read rejected material does it (Global Constraints).

- [ ] **Step 1: Branch, and find the candidate scripts by their constants.**

```bash
git switch dev && git pull --ff-only && git switch -c docs/re-<issue>-login
mkdir -p re/m1/login
grep -rlF --include='*.dis' -e '"net_client_create"' -e '"servername"' -e '"server_name"' \
  -e '"server-name"' -e '"server_locale"' -e 'GetGFChooseServerFrame' -e 'FAILED TO FETCH' \
  -e '"ServerList"' -e '"serverlist"' -e '"server_list"' re/m1/disasm | sort > re/m1/login/candidates.txt
wc -l re/m1/login/candidates.txt
```

Expected: a short list including `addons/Login/*.luac.dis`. If it holds more than about 40 files, rank them by how many of these constants each holds, and read the top ones first.

- [ ] **Step 2: Trace the connect path in the scripts.** Start from the calls to `net_client_create` (the binding that creates the game connection), and walk back to where its host and port arguments come from:
  - a server-list response, a file the client reads (`servername`), a flag value, a table in `load.config` or `data/`, or a constant.

  Read the decompiled `.lua` files for flow and the `.dis` listings for exact constants and calls. Record each step in `re/m1/login/trace.md` (local): script path, function (by its disassembly path, such as `0.3.1`), and constants. Also note:
  - what the server-list fetch requests and from where (URL, host or file);
  - what the error "FAILED TO FETCH GAMEFORGE SERVER LIST!" is conditional on;
  - what `GetGFChooseServerFrame` shows.

- [ ] **Step 3: Read the native side in Ghidra** (read-only). For the strings `--server-name`, `--server_locale`, `--net` and `servername`:
  - run `search_strings` → `get_xrefs_to` → `force_decompile` on the reading function;
  - record where each value goes. Does it reach Lua, and under which global or field name? Does it carry a host:port?

  Tag code readings Inferred, and cite addresses.

- [ ] **Step 4: Decide the input for Task 10**, and check it against the gates:
  - **Allowed:** an input the client already reads (a flag, a file at the install root, a config value, a hosts name). This is spec §5.3 option 1, or option 3 for a name.
  - **Not allowed:** if the only way is to patch the client, change a file it ships, or inject code. Stop: open a `Decision gate` issue with what was found and the options, tell the owner, and continue with Task 9 while waiting.
  - **German UI:** if the trace shows why the UI is German (locale selection, `load.config [default]`, `--server_locale`, or the mocked SDK's region), record it. Otherwise list it as Unknown.

- [ ] **Step 5: Write `docs/protocol/login.md`.** Prose and tables only, with no script text. Every finding is tagged Observed / Inferred / Unknown, with evidence: the script path and function path, or a Ghidra address.
  - `# Login: how the client finds its server`, with a summary of two or three sentences.
  - `## 1. Inputs the client reads`: flags, files and config, each with where it is read (native address or script) and what it controls.
  - `## 2. The server list`: what is fetched, from where, in which format, and when the failure message appears.
  - `## 3. Pointing the client at our stub`: the exact input for Task 10, as a table (input, value for the stub, why), and the expected ports.
  - `## 4. The German interface`: an answer, or Unknown.
  - `## 5. Open questions`.
  - `## 6. Verification`: filled in by Task 10.
  - `## Evidence index`.

Add to `docs/README.md`:

```markdown
| [`protocol/login.md`](protocol/login.md) | M1: how the client finds its server, and the first connection to our stub. |
```

- [ ] **Step 6: Commit and merge.**

```bash
scripts/check-forbidden-files.sh
git add docs/protocol/login.md docs/README.md
git commit -m "docs(re): trace how the client finds its server" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

Wait for CI, then squash-merge.

---

### Task 9: Tracked client runner, AGT watchdog, pinned GE-Proton

**Files:**
- Create: `scripts/spike/run-client.sh`, `scripts/spike/watchdog.py`, `scripts/spike/test-watchdog.py`
- Modify:
  - `.github/workflows/ci.yml`
  - `scripts/versions.env`
  - `scripts/bootstrap.sh`
  - `CLAUDE.md` (the pinned table and the "Client runs" bullet)
  - `docs/development.md`
  - `docs/re/README.md` ("Running the client")
  - `docs/re/sources.md`

**Interfaces:**
- Consumes:
  - `scripts/spike/isolated-run.sh`, with `--no-root --x11 N --cpu-topology --hosts --bind --ro-bind`;
  - `fenji-stub` (`--listen ADDR`... `--out DIR`);
  - the local `re/spike/run.sh` and `re/spike/watchdog.py`. These are the revisions runs 8–9 used; `watchdog.py` has SHA-256 `f0792415…`.
- Produces, for Task 10:
  - `scripts/spike/run-client.sh setup [--display N]`
  - `scripts/spike/run-client.sh run NAME [--hosts F] [--ports "P…"] [--timeout S] [--display N] [--channels W] [--env V=X]… [--client-file NAME=SRC]… -- ARGS`, which exits 0, 3 (gate event), 4 or 5
  - `GE_PROTON_VERSION` and `GE_PROTON_SHA256_x86_64` in `scripts/versions.env`
- Closes #17 (Task 0 marked it as a duplicate of this task's issue).

- [ ] **Step 1: Branch, and copy the watchdog in.**

```bash
git switch dev && git pull --ff-only && git switch -c feat/re-<issue>-client-runner
sha256sum re/spike/watchdog.py   # expect f0792415… (runs 8–9's revision)
cp re/spike/watchdog.py scripts/spike/watchdog.py
```

In `scripts/spike/watchdog.py`'s docstring:
- change the first line to `"""Driver watchdog for scripts/spike/run-client.sh (M1 Task 9; first used locally in M0's runs 5–9).`;
- replace the S1 bullet with:
  `S1 a *.sys under S that isn't in the baseline, or a baseline one whose identity changed: a regular file's size, mtime or inode, or a symlink's target or the target file's size, mtime or inode (sys_identity; a link merely re-created by Proton's prefix re-sync is not a change); any *.sys in the sandbox's own tmpfs mounts, seen through /proc/<pid>/root of a sandbox process.`

- [ ] **Step 2: Write the failing watchdog tests**, `scripts/spike/test-watchdog.py`:

```python
#!/usr/bin/env python3
"""Tests for scripts/spike/watchdog.py's pure rules: the S1 *.sys comparison and the S3 Wine-log
rules. Stdlib only, no sandbox and no Wine: python3 -I scripts/spike/test-watchdog.py"""
import importlib.util
import pathlib
import sys
import tempfile
import time
import unittest

HERE = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("watchdog", HERE / "watchdog.py")
wd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(wd)


class S1(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        (self.root / "proton").mkdir()
        (self.root / "pfx").mkdir()
        self.target = self.root / "proton" / "winebth.sys"
        self.target.write_bytes(b"Wine builtin DLL")
        self.link = self.root / "pfx" / "winebth.sys"
        self.link.symlink_to(self.target)
        self.regular = self.root / "pfx" / "mountmgr.sys"
        self.regular.write_bytes(b"Wine builtin DLL")
        self.base = wd.scan_sys(str(self.root))

    def tearDown(self):
        self._tmp.cleanup()

    def rules(self):
        return [f[0] for f in wd.compare_sys(self.base, wd.scan_sys(str(self.root)))]

    def test_nothing_changed(self):
        self.assertEqual(self.rules(), [])

    def test_recreated_link_with_the_same_target_is_quiet(self):
        time.sleep(0.01)
        self.link.unlink()
        self.link.symlink_to(self.target)  # what Proton's prefix re-sync does (look-1)
        self.assertEqual(self.rules(), [])

    def test_link_to_a_different_file_triggers(self):
        other = self.root / "proton" / "agt.sys"
        other.write_bytes(b"MZ driver")
        self.link.unlink()
        self.link.symlink_to(other)
        self.assertIn("S1 changed .sys file", self.rules())

    def test_changed_link_target_triggers(self):
        time.sleep(0.01)
        self.target.write_bytes(b"MZ a different driver")
        self.assertIn("S1 changed .sys file", self.rules())

    def test_new_driver_file_triggers(self):
        (self.root / "pfx" / "agt.sys").write_bytes(b"MZ")
        self.assertEqual(self.rules(), ["S1 new .sys file"])

    def test_rewritten_regular_file_triggers(self):
        time.sleep(0.01)
        self.regular.write_bytes(b"MZ something else")
        self.assertEqual(self.rules(), ["S1 changed .sys file"])


CLIENT = r'1.000:0020:0024:trace:process:CreateProcessInternalW app (null) cmdline L"\"Z:\\c\\bin64\\gujianol.exe\""'
AGT_LOADDRIVER = r'2.000:0098:00a4:trace:ntoskrnl:ZwLoadDriver (L"\\Registry\\Machine\\System\\CurrentControlSet\\Services\\AGT")'


class S3(unittest.TestCase):
    def scan(self, *lines):
        rules = wd.LogRules(["winebth", "mountmgr"])
        trig, notes = rules.scan(("\n".join(lines) + "\n").encode())
        return [t[0] for t in trig], [n[0] for n in notes]

    def test_split_winebth_line_after_the_client_is_a_note(self):  # run 7
        trig, notes = self.scan(
            CLIENT,
            "2.000:0098:00a4:err:ntoskrnl:ZwLoadDriver flags 0x1, load_path 0000000000000000, "
            "name 0000000000A1F840, base 0000000000A1F838.",
            '2.000:0098:00a4:trace:module:LdrGetDllHandleEx L"ntdll.dll" -> 00006FFFFFEF0000 (load path (null))',
            r'failed to create driver L"\\Registry\\Machine\\System\\CurrentControlSet\\Services\\winebth": c0000142',
        )
        self.assertEqual(trig, [])
        self.assertEqual(notes, ["LoadDriver for baseline service winebth (message split across lines)"])

    def test_loaddriver_before_the_client_exists_is_a_note(self):
        trig, notes = self.scan(AGT_LOADDRIVER.replace("2.000:", "1.000:", 1))
        self.assertEqual(trig, [])
        self.assertTrue(notes[0].startswith("LoadDriver logged at 1.000, before the client's process creation"))

    def test_non_baseline_loaddriver_after_the_client_triggers(self):
        trig, _ = self.scan(CLIENT, AGT_LOADDRIVER)
        self.assertEqual(trig, ["S3 LoadDriver for non-baseline service agt"])

    def test_native_sys_load_triggers(self):
        trig, _ = self.scan(
            r'2.000:0134:0138:trace:loaddll:build_module Loaded L"C:\\windows\\system32\\drivers\\agt.sys" '
            "at 0000000140000000: native"
        )
        self.assertEqual(trig, ["S3 .sys module loaded native"])

    def test_pe_resource_lookup_triggers(self):
        trig, _ = self.scan('2.000:0134:0138:trace:resource:FindResourceExW 140000000 L"PE" #0090 0')
        self.assertEqual(trig, ['S3 FindResource of type "PE" (embedded driver resource type)'])

    def test_driver_strings_trigger(self):
        trig, _ = self.scan('2.000:0134:0138:warn:debugstr:OutputDebugStringA "ProcessProtect ready"')
        self.assertEqual(trig, ["S3 driver string in log"])

    def test_createservice_for_an_unknown_service_triggers(self):
        trig, _ = self.scan('2.000:0134:0138:trace:service:CreateServiceW 0000000000000001 L"AGT" L"AGT"')
        self.assertEqual(trig, ["S3 CreateService for non-baseline service 'AGT'"])


if __name__ == "__main__":
    sys.exit(0 if unittest.main(exit=False, verbosity=2).result.wasSuccessful() else 1)
```

Run: `python3 -I scripts/spike/test-watchdog.py`
Expected:
- `S1.test_recreated_link_with_the_same_target_is_quiet` fails with `S1 changed .sys file`. That is the look-1 false positive.
- All S1 tests error with `AttributeError: module 'watchdog' has no attribute 'compare_sys'`.
- The S3 tests pass, since they pin behaviour that already exists.

- [ ] **Step 3: Fix S1 in `scripts/spike/watchdog.py`.** Add `import stat` to the imports. Replace `scan_sys` with the three functions below:

```python
def sys_identity(path):
    """What a *.sys entry is, for comparing snapshots. A symlink is its target plus the target
    file's size, mtime and inode: Proton re-creates its links to Wine's built-in drivers at every
    prefix re-sync, which renews the link itself but not what it points to (the owner's run look-1,
    2026-10-12). A regular file is its own size, mtime and inode."""
    st = os.lstat(path)
    if stat.S_ISLNK(st.st_mode):
        target = os.readlink(path)
        try:
            t = os.stat(path)
        except OSError:
            return ["link", target, None, None, None]
        return ["link", target, t.st_size, t.st_mtime_ns, t.st_ino]
    return ["file", None, st.st_size, st.st_mtime_ns, st.st_ino]


def scan_sys(root, prune=()):
    found = {}
    for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
        if prune:
            dirnames[:] = [d for d in dirnames if os.path.join(dirpath, d) not in prune]
        for f in filenames:
            if f.lower().endswith(".sys"):
                p = os.path.join(dirpath, f)
                try:
                    found[p] = sys_identity(p)
                except OSError:
                    continue
    return found


def compare_sys(base, cur):
    """S1 findings between two scan_sys() snapshots: new *.sys entries, and changed ones."""
    out = []
    for p, ident in sorted(cur.items()):
        if p not in base:
            out.append(("S1 new .sys file", p, ident))
        elif base[p] != ident:
            out.append(("S1 changed .sys file", p, {"before": base[p], "after": ident}))
    return out
```

In `cmd_watch`, replace the body of `check_sys()` so it uses the shared comparison:

```python
    def check_sys():
        out = compare_sys(base_sys, scan_sys(a.root))
        if init:
            for p, st in sandbox_root_scan().items():
                out.append(("S1 .sys file in a sandbox tmpfs mount", p, st))
        return out
```

Run: `python3 -I scripts/spike/test-watchdog.py`
Expected: `Ran 13 tests … OK`.

- [ ] **Step 4: Write `scripts/spike/run-client.sh`**, ported from `re/spike/run.sh`. It adds a `setup` command, runs GE-Proton from the pinned tools directory, takes the display and client-file inputs as options, and uses the repo's own wrapper rather than a frozen copy.

```bash
#!/usr/bin/env bash
# Runs the unmodified pinned SoLO client once inside scripts/spike/isolated-run.sh, with
# fenji-stub listening and the AGT driver watchdog (watchdog.py) armed (design spec §4 B; M1 spec
# §4, W4).
#
#   scripts/spike/run-client.sh setup [--display N]
#       Makes the git-ignored run workspace re/runs/: client/, a reflink copy of
#       $FENJI_CLIENT_PATH (a full copy is refused); a Wine prefix in compat/, created with Wine's
#       own cmd.exe; and empty home/, steamclient/ and hosts-empty.
#   scripts/spike/run-client.sh run NAME [--hosts FILE] [--ports "P1 P2"] [--timeout S]
#       [--display N] [--channels WINEDEBUG] [--env VAR=VALUE]... [--client-file NAME=SRC]...
#       -- [client args...]
#       One run: evidence in re/runs/out/NAME/, stub logs in re/stub-logs/NAME/; NAME must be new.
#       --client-file puts SRC into the run's client copy as NAME at the install root, an input
#       the client reads, for this run only. It never replaces a file the copy already has.
#
# Every run uses isolated-run.sh --no-root --x11 N --cpu-topology (CLAUDE.md "Client runs"), the
# pinned GE-Proton's own `proton run` (umu's runtime container fails in the sandbox), lavapipe, and
# Wine channels that record lookups, sockets, modules and driver activity. The dev-only
# --gf-sdk=mocked (gate #11) and --fs=data:_index:_index are the caller's to pass.
#
# Exit: 0 the run finished; 3 GATE EVENT (the watchdog stopped the run: stop all runs, report);
# 4 the watchdog failed; 5 the watchdog never armed (the client was not started); 1 or 2 setup or
# usage errors.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd -P)
REPO=$(cd -- "$HERE/../.." && pwd -P)
# shellcheck source=../versions.env
. "$REPO/scripts/versions.env"
WORK="$REPO/re/runs"
WRAP="$HERE/isolated-run.sh"
WATCHDOG="$HERE/watchdog.py"
PROTON="${FENJI_GE_PROTON:-$HOME/.local/share/fenji/tools/$GE_PROTON_VERSION}"
STUBBIN="$REPO/target/release"
LAVAPIPE=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
CHANNELS_DEFAULT=+timestamp,+pid,+tid,+threadname,+seh,+debugstr,+loaddll,+winsock,+module,+process,+service,+profile,warn+file

usage() { sed -n '2,24p' "$0" >&2; exit 2; }
die() { echo "run-client: $*" >&2; exit 1; }
need_display() { [ -S "/tmp/.X11-unix/X$1" ] || die "no nested display :$1 (see isolated-run.sh, \"Display\")"; }

cmd_setup() {
  local display=99
  while [ $# -gt 0 ]; do
    case "$1" in
      --display) [ $# -ge 2 ] || usage; display=$2; shift 2 ;;
      *) usage ;;
    esac
  done
  [ -n "${FENJI_CLIENT_PATH:-}" ] || die "set FENJI_CLIENT_PATH to the pinned client copy (docs/re/pinned-build.md)"
  [ -f "$FENJI_CLIENT_PATH/bin64/gujianol.exe" ] || die "no bin64/gujianol.exe under $FENJI_CLIENT_PATH"
  [ ! -e "$WORK/client" ] || die "$WORK/client exists already; remove re/runs/ to start over"
  [ -x "$PROTON/proton" ] || die "GE-Proton not found at $PROTON (run scripts/bootstrap.sh)"
  need_display "$display"
  mkdir -p "$WORK/client" "$WORK/compat" "$WORK/home" "$WORK/steamclient" "$WORK/out" "$REPO/re/stub-logs"
  : > "$WORK/hosts-empty"
  if ! cp -a --reflink=always "$FENJI_CLIENT_PATH/." "$WORK/client/"; then
    rm -rf "$WORK/client"
    die "the reflink copy failed (it needs a copy-on-write filesystem such as btrfs); refusing a full copy"
  fi
  # The prefix is created now, by Wine's own cmd.exe. A prefix appearing during a client run would
  # look like new driver files to the watchdog.
  "$WRAP" --no-root --x11 "$display" --cpu-topology --bind "$WORK" --ro-bind "$PROTON" -- \
    env HOME="$WORK/home" STEAM_COMPAT_DATA_PATH="$WORK/compat" \
    STEAM_COMPAT_CLIENT_INSTALL_PATH="$WORK/steamclient" SteamGameId=1418100 SteamAppId=1418100 \
    PROTON_USE_XALIA=0 VK_ICD_FILENAMES="$LAVAPIPE" \
    timeout -k 10 300 python3 "$PROTON/proton" run cmd.exe /c exit > "$WORK/prefix-init.log" 2>&1 || true
  [ -f "$WORK/compat/pfx/system.reg" ] || die "the Wine prefix wasn't created; see $WORK/prefix-init.log"
  echo "workspace ready: $WORK"
}

cmd_run() {
  [ $# -ge 1 ] || usage
  local run=$1
  shift
  local hosts="$WORK/hosts-empty" ports=7804 tmo=300 display=99 channels=$CHANNELS_DEFAULT
  local envs=() client_files=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --hosts) [ $# -ge 2 ] || usage; hosts=$(realpath -- "$2"); shift 2 ;;
      --ports) [ $# -ge 2 ] || usage; ports=$2; shift 2 ;;
      --timeout) [ $# -ge 2 ] || usage; tmo=$2; shift 2 ;;
      --display) [ $# -ge 2 ] || usage; display=$2; shift 2 ;;
      --channels) [ $# -ge 2 ] || usage; channels=$2; shift 2 ;;
      --env) [ $# -ge 2 ] || usage; envs+=("$2"); shift 2 ;;
      --client-file) [ $# -ge 2 ] || usage; client_files+=("$2"); shift 2 ;;
      --) shift; break ;;
      *) usage ;;
    esac
  done
  case "$run" in '' | */* | .*) die "bad run name: '$run'" ;; esac
  case "$tmo" in '' | *[!0123456789]*) die "--timeout takes whole seconds" ;; esac
  case "$display" in '' | *[!0123456789]*) die "--display takes a display number" ;; esac
  local OUT="$WORK/out/$run" STUB="$REPO/re/stub-logs/$run" PFX="$WORK/compat/pfx"
  if [ ! -d "$WORK/client/bin64" ] || [ ! -f "$PFX/system.reg" ]; then die "no workspace; run: $0 setup"; fi
  if [ -e "$OUT" ] || [ -e "$STUB" ]; then die "run $run exists already; pick a new name"; fi
  need_display "$display"
  [ -f "$hosts" ] || die "hosts file missing: $hosts"
  [ -x "$PROTON/proton" ] || die "GE-Proton not found at $PROTON (run scripts/bootstrap.sh)"
  [ -x "$STUBBIN/fenji-stub" ] || die "build the stub first: cargo build --release -p fenji-stub"
  if pgrep -f 'winedevice\.exe|wineserver' >/dev/null; then
    die "Wine processes are already running on the host; stop them first"
  fi
  local spec name f
  for spec in "${client_files[@]}"; do
    name=${spec%%=*}
    case "$name" in '' | */* | . | ..) die "--client-file NAME must be a plain file name, not '$name'" ;; esac
    [ -f "${spec#*=}" ] || die "--client-file source missing: ${spec#*=}"
    [ ! -e "$WORK/client/$name" ] || die "--client-file $name would replace a file the client copy has"
  done
  for f in gujianol.log Log_gujianol.htm; do
    [ ! -e "$WORK/client/bin64/$f" ] || die "leftover $f in the client copy's bin64; move it first"
  done
  mkdir -p "$OUT" "$STUB"

  # --- before: inputs, client files, AGT baseline
  {
    echo "run: $run"
    echo "date: $(date -Is)"
    echo "client args: $*"
    echo "extra env: ${envs[*]-}"
    echo "client files: ${client_files[*]-}"
    echo "stub ports: $ports"
    echo "timeout: $tmo"
    echo "display: :$display"
    echo "wine channels (WINEDEBUG, with PROTON_LOG=1): $channels"
    echo "GE-Proton: $PROTON"
    echo "sandbox: isolated-run.sh --no-root --x11 $display --cpu-topology (sha256 $(sha256sum "$WRAP" | cut -c1-16)…)"
    echo "driver watchdog: watchdog.py sha256 $(sha256sum "$WATCHDOG" | cut -c1-16)…"
    echo "hosts file: $hosts"
    echo "--- hosts content"
    cat "$hosts"
  } > "$OUT/inputs.txt"
  for spec in "${client_files[@]}"; do
    cp -- "${spec#*=}" "$WORK/client/${spec%%=*}"
  done
  find "$PFX" -iname '*.sys' -printf '%P\t%y\t%l\t%s\t%T@\n' | sort > "$OUT/sys-before.tsv"
  grep -o '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\[^]\\]*' "$PFX/system.reg" | sort -u > "$OUT/services-before.txt" || true
  mkdir -p "$OUT/reg-before" && cp -p "$PFX/system.reg" "$PFX/user.reg" "$OUT/reg-before/"
  python3 -I "$WATCHDOG" baseline --root "$WORK" --out "$OUT"
  touch "$OUT/.start"

  # --- host-side screenshots while the run is active
  (
    start=$(date +%s)
    for t in 5 10 30 60 90 120 150 180 210 240 270 295; do
      [ "$t" -le "$tmo" ] || break
      now=$(date +%s)
      sl=$((start + t - now))
      [ "$sl" -le 0 ] || sleep "$sl"
      [ -e "$OUT/.running" ] || break
      import -display ":$display" -window root "$OUT/screen-${t}s.png" 2>>"$OUT/screenshot.err" || true
    done
  ) &
  local shots=$!
  touch "$OUT/.running"

  # --- the sandboxed run: stub + client in one invocation, started once the watchdog has armed
  set +e
  # shellcheck disable=SC2016 # the bash -c script expands its own positional parameters
  "$WRAP" --no-root --x11 "$display" --cpu-topology --hosts "$hosts" --bind "$WORK" \
    --bind "$REPO/re/stub-logs" --ro-bind "$STUBBIN" --ro-bind "$PROTON" -- \
    env HOME="$WORK/home" STEAM_COMPAT_DATA_PATH="$WORK/compat" \
    STEAM_COMPAT_CLIENT_INSTALL_PATH="$WORK/steamclient" SteamGameId=1418100 SteamAppId=1418100 \
    PROTON_LOG_DIR="$OUT" PROTON_USE_XALIA=0 PROTON_LOG=1 WINEDEBUG="$channels" DXVK_LOG_PATH="$OUT" \
    VK_ICD_FILENAMES="$LAVAPIPE" "${envs[@]}" \
    bash -c '
      W=$1 OUT=$2 STUB=$3 STUBBIN=$4 PORTS=$5 TMO=$6 PROTON=$7
      shift 7
      for _ in $(seq 200); do [ -e "$OUT/.go" ] && break; sleep 0.05; done
      [ -e "$OUT/.go" ] || { echo "watchdog never armed; client not started" >&2; exit 98; }
      la=()
      for p in $PORTS; do la+=(--listen "127.0.0.1:$p"); done
      "$STUBBIN/fenji-stub" "${la[@]}" --out "$STUB" > "$OUT/stub.out" 2>&1 &
      stub=$!
      sleep 1
      cd "$W/client/bin64" || exit 99
      timeout -k 10 "$TMO" python3 "$PROTON/proton" run "$W/client/bin64/gujianol.exe" "$@" > "$OUT/proton.out" 2>&1
      rc=$?
      echo "$rc" > "$OUT/exit-code"
      WINEPREFIX="$W/compat/pfx" "$PROTON/files/bin/wineserver" -k 2>/dev/null
      sleep 2
      kill "$stub" 2>/dev/null
      wait "$stub" 2>/dev/null
      exit "$rc"
    ' _ "$WORK" "$OUT" "$STUB" "$STUBBIN" "$ports" "$tmo" "$PROTON" "$@" > "$OUT/sandbox.out" 2>&1 &
  local spid=$!
  python3 -I "$WATCHDOG" watch --root "$WORK" --out "$OUT" --sandbox-pid "$spid" \
    --log "$OUT/steam-1418100.log" --bind "$REPO/re/stub-logs" --bind "$STUBBIN" \
    2>> "$OUT/watchdog.err" &
  local wdpid=$!
  # Whichever ends first. If it is the watchdog (gate event, failure, or killed), the sandbox must
  # not outlive it.
  local first="" frc wrc wdrc
  wait -n -p first "$spid" "$wdpid"
  frc=$?
  if [ "$first" = "$wdpid" ]; then
    wdrc=$frc
    kill -KILL "$spid" 2>/dev/null
    wait "$spid"
    wrc=$?
  else
    wrc=$frc
    wait "$wdpid"
    wdrc=$?
  fi
  set -e
  rm -f "$OUT/.running" "$OUT/.go"
  wait "$shots" 2>/dev/null || true
  echo "$wrc" > "$OUT/sandbox-exit-code"
  echo "$wdrc" > "$OUT/watchdog-exit-code"
  date -Is > "$OUT/ended"

  # --- after: evidence
  [ ! -f "$OUT/steam-1418100.log" ] || mv "$OUT/steam-1418100.log" "$OUT/wine.log"
  for f in gujianol.log Log_gujianol.htm; do
    [ ! -e "$WORK/client/bin64/$f" ] || mv "$WORK/client/bin64/$f" "$OUT/$f"
  done
  # Entries the client creates at its install root (settings/, ExportFace/, ExportPrefab/) and this
  # run's --client-file inputs move into the evidence, so every run starts from the same copy.
  local e
  for e in "$WORK/client"/* "$WORK/client"/.[!.]*; do
    [ -e "$e" ] || continue
    case "${e##*/}" in
      bin64 | data | tools) ;;
      *) mkdir -p "$OUT/client-files" && mv "$e" "$OUT/client-files/" ;;
    esac
  done
  find "$PFX" -iname '*.sys' -printf '%P\t%y\t%l\t%s\t%T@\n' | sort > "$OUT/sys-after.tsv"
  diff "$OUT/sys-before.tsv" "$OUT/sys-after.tsv" > "$OUT/sys-diff.txt" || true
  grep -o '^\[System\\\\ControlSet00[0-9]\\\\Services\\\\[^]\\]*' "$PFX/system.reg" | sort -u > "$OUT/services-after.txt" || true
  diff "$OUT/services-before.txt" "$OUT/services-after.txt" > "$OUT/services-diff.txt" || true
  mkdir -p "$OUT/reg-after" && cp -p "$PFX/system.reg" "$PFX/user.reg" "$OUT/reg-after/"
  local r
  for r in system user; do
    diff "$OUT/reg-before/$r.reg" "$OUT/reg-after/$r.reg" > "$OUT/reg-$r.diff" || true
  done
  find "$WORK/client" "$PFX/drive_c" "$WORK/home" -newer "$OUT/.start" -type f -not -path '*/mesa_shader_cache/*' \
    -printf '%TT\t%s\t%p\n' 2>/dev/null | sort > "$OUT/new-files.txt" || true
  echo "run $run done: proton exit $(cat "$OUT/exit-code" 2>/dev/null || echo '?'), sandbox exit $wrc, watchdog exit $wdrc"
  case "$wdrc" in
    0) ;;
    3) echo "GATE EVENT: the driver watchdog stopped the run; see $OUT/watchdog-trigger.txt. Stop all runs." >&2; exit 3 ;;
    *) echo "WATCHDOG FAILURE (exit $wdrc); see $OUT/watchdog.log and watchdog.err" >&2; exit 4 ;;
  esac
  [ "$wrc" != 98 ] || { echo "the watchdog never armed; the client was not started" >&2; exit 5; }
}

case "${1:-}" in
  setup) shift; cmd_setup "$@" ;;
  run) shift; cmd_run "$@" ;;
  *) usage ;;
esac
```

Run:

```bash
chmod +x scripts/spike/run-client.sh scripts/spike/test-watchdog.py
shellcheck scripts/spike/run-client.sh
scripts/spike/run-client.sh; echo "exit $?"
```

Expected: ShellCheck is clean. The bare call prints the usage, lines 2–24, and exits 2.

- [ ] **Step 5: Pin GE-Proton.** In `scripts/versions.env`, under "Steam / Proton tooling":

```bash
# GE-Proton: the Proton build every sandboxed client run uses (Linux x86_64 only).
GE_PROTON_VERSION="GE-Proton11-7"
GE_PROTON_SHA256_x86_64="c5448b76a230384e2d7bc6beb5ccb97bafb7e2c3b6c527cb03a1a546bbcb00a0"   # GitHub release digest
```

In `scripts/bootstrap.sh`:
- next to `UMU_DIR`, add `GE_PROTON_DIR="$TOOLS_DIR/$GE_PROTON_VERSION"`;
- in `check_all()`, inside the Linux block after the umu report, add `report GE-Proton "$GE_PROTON_VERSION" "$([ -x "$GE_PROTON_DIR/proton" ] && echo "$GE_PROTON_VERSION")"`;
- in section 5's Linux block, after the umu lines, add:

```bash
  if [ "$(uname -m)" = x86_64 ] && [ ! -x "$GE_PROTON_DIR/proton" ]; then
    log "GE-Proton $GE_PROTON_VERSION (SHA-256 verified)"
    tmp=$(mktemp -d)
    download_verified \
      "https://github.com/GloriousEggroll/proton-ge-custom/releases/download/${GE_PROTON_VERSION}/${GE_PROTON_VERSION}-x86_64.tar.gz" \
      "$GE_PROTON_SHA256_x86_64" "$tmp/ge-proton.tar.gz"
    tar -xzf "$tmp/ge-proton.tar.gz" -C "$TOOLS_DIR"
    rm -rf "$tmp"
  fi
```

Add to `CLAUDE.md`'s pinned table, after umu-launcher, in the same commit:

```markdown
| GE-Proton | GE-Proton11-7 | Linux x86_64, SHA-256 pinned, `~/.local/share/fenji/tools/GE-Proton11-7`; every sandboxed client run (`scripts/spike/run-client.sh`) |
```

In `CLAUDE.md`'s "Client runs" bullet, after `(loopback only; …)`, add: `Use scripts/spike/run-client.sh, which adds fenji-stub and the AGT driver watchdog.`

In `docs/development.md`'s tools bullet, add: `GE-Proton (Linux x86_64) lands in ~/.local/share/fenji/tools/GE-Proton11-7.`

In `docs/re/README.md` "Running the client", replace the pointer to the local runner (#17) with: `scripts/spike/run-client.sh setup once, then run-client.sh run NAME … -- ARGS. The flags that reach the login scene are in spike-0 §8; --gf-sdk=mocked is dev-only (gate #11).`

Add a `docs/re/sources.md` "Used" row:

```markdown
| [GE-Proton](https://github.com/GloriousEggroll/proton-ge-custom) GE-Proton11-7 | BSD-3-Clause (Proton) with Wine's LGPL and other bundled licences | Yes: a public Proton fork | External runtime for sandboxed client runs, never redistributed (M1 Task 9) | 2026-10-12 |
```

Run: `scripts/bootstrap.sh`, then `scripts/bootstrap.sh --check | grep -E 'GE-Proton|unluac'`.
Expected: both report their pinned versions, and `~/.local/share/fenji/tools/GE-Proton11-7/proton` exists.

- [ ] **Step 6: Add the watchdog tests to CI.** In `.github/workflows/ci.yml`'s `repo-hygiene` job, after "Forbidden-file check tests", add:

```yaml
      - name: Watchdog tests
        run: python3 -I scripts/spike/test-watchdog.py
```

- [ ] **Step 7: Acceptance run.** One client run, re-doing M0's run 8 through the new runner. This is RE work, for a clean session only.
  - **Preconditions:** Steam not running; `/tmp/.X11-unix/X99` exists. The controller may start `Xwayland :99 -geometry 1280x720 -ac -nolisten tcp -noreset -extension MIT-SHM -shm &`, with the owner's standing permission.

```bash
cargo build --release -p fenji-stub
FENJI_CLIENT_PATH=~/fenji-client scripts/spike/run-client.sh setup
scripts/spike/run-client.sh run m1-accept --timeout 180 -- --gf-sdk=mocked --fs=data:_index:_index; echo "exit $?"
```

Expected: exit 0, the watchdog's exit code is 0, and `re/runs/out/m1-accept/screen-120s.png` shows the login scene as in spike-0 8.17.

If the exit is 3, inspect `watchdog-trigger.txt` before anything else:
- If it's a new false-positive pattern, add its regression test to `test-watchdog.py` and fix the rule in this PR.
- If it isn't a false positive, it's a gate event: stop and report to the owner.

- [ ] **Step 8: Commit and merge.**

```bash
scripts/check-forbidden-files.sh --staged
git add scripts/spike/run-client.sh scripts/spike/watchdog.py scripts/spike/test-watchdog.py .github/workflows/ci.yml \
  scripts/versions.env scripts/bootstrap.sh CLAUDE.md docs/development.md docs/re/README.md docs/re/sources.md
git commit -m "feat(re): track the client runner and AGT watchdog, and pin GE-Proton" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

This task touches the AGT gate's run-time guard (Review Focus 5). Before merging, dispatch one fresh reviewer (`general-purpose`, `opus`) over the PR diff. Its brief: Review Focus 5, the S1 and S3 rules, "can a real driver load or a new driver file slip past the watchdog", and the runner's input handling (`--client-file`, run names, hosts). Fix what it finds, then wait for CI and squash-merge.

---

### Task 10: First connection: the real client reaches `fenji-stub`

**Files:**
- Modify: `docs/protocol/login.md` (§6 Verification, the evidence index)
- Local only: `re/runs/out/m1-c*/`, `re/stub-logs/m1-c*/`, `re/m1/connect/`

**Interfaces:**
- Consumes Task 8's §3 input and ports, and Task 9's `run-client.sh`.
- Produces Done-when 3, recorded as evidence in `docs/protocol/login.md` §6: a stub session from the real client.

This is RE work, and every run is a client run: clean session only, inside `run-client.sh`. **Cap: 4 runs**, one variable at a time.

- [ ] **Step 1: Branch and preconditions.**

```bash
git switch dev && git pull --ff-only && git switch -c docs/re-<issue>-first-connection
pgrep -x steam && echo "quit Steam first"; test -S /tmp/.X11-unix/X99 || echo "start the nested display (Task 9 Step 7)"
mkdir -p re/m1/connect
```

- [ ] **Step 2: Prepare the inputs from `login.md` §3:**
  - a hosts file `re/m1/connect/hosts` mapping every name §3 lists to `127.0.0.1`;
  - any file inputs, for example `re/m1/connect/servername`, holding exactly the value §3 gives;
  - the ports list (§3's ports, all ≥ 1024).

  If §3 needs a port below 1024: under `--no-root` the stub can't bind it, so stop and ask the owner. Options are the sandbox's root mode, which grants `CAP_NET_BIND_SERVICE` but which umu/Proton may refuse, or a different input.

- [ ] **Step 3: Run 1.** `--client-file` covers inputs at the install root, `--hosts` covers names, and flags go after `--`. If §3's input lives anywhere other than the install root, extend `run-client.sh` first, in the same PR, rather than editing the client copy by hand.

```bash
scripts/spike/run-client.sh run m1-c1 --timeout 240 --hosts re/m1/connect/hosts --ports "<§3 ports>" \
  --client-file servername=re/m1/connect/servername \
  -- --gf-sdk=mocked --fs=data:_index:_index; echo "exit $?"
ls re/stub-logs/m1-c1/
```

Expected: exit 0, and at least one session file in `re/stub-logs/m1-c1/`. A session with 0 bytes passes, because spike-0 4.23–4.28 reads the server as speaking first.

- [ ] **Step 4: If there's no session, read the evidence before changing anything.**

```bash
grep -E 'connect|getaddrinfo|gethostbyname|socket' re/runs/out/m1-c1/wine.log | grep -v ':0028:' | head -40
grep -iE 'server|login|fetch' re/runs/out/m1-c1/gujianol.log | head -40
```

Change one variable per run, at most runs 2–4, each recorded in `re/m1/connect/runs.md` (local) with its reason. If the evidence shows the input wasn't read at all, go back to Task 8's trace rather than guessing.

- [ ] **Step 5: Record the outcome.** In `docs/protocol/login.md` §6, write a run table: run, change, stub sessions (count; for each, the port, the length and the first 16 bytes), names looked up, ports tried, AGT watchdog result, and exit. Then Observed / Inferred / Unknown bullets, with the evidence keys `re/runs/out/m1-c*/` and `re/stub-logs/m1-c*/`.

If no run connects within the cap: write what was learned, open an `RE investigation` issue for the remaining input, tell the owner, and mark Done-when 3 as not met. Task 14 carries it.

- [ ] **Step 6: Commit and merge.**

```bash
git add docs/protocol/login.md
git commit -m "docs(re): record the client's first connection to the stub" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

Wait for CI, then squash-merge.

---

### Task 11: Script census: server-side logic in the embedded scripts

**Files:**
- Create:
  - `crates/fenji-lua/src/census.rs`
  - `crates/fenji-lua/tests/census.rs`
  - `docs/re/m1-script-census.md`
- Modify:
  - `crates/fenji-lua/src/lib.rs`
  - `tools/luatool/src/commands.rs`
  - `tools/luatool/src/main.rs`
  - `tools/luatool/tests/cli.rs`
  - `docs/README.md`

**Interfaces:**
- Consumes Tasks 4–7.
- Produces:
  - `fenji_lua::census(&Chunk) -> Counts`
  - `fenji_lua::Counts { chunks, functions, instructions, reads_is_server, reads_is_client, server_only, client_only }` (all `u64`; `AddAssign`, `Copy`)
  - `luatool census --out DIR`, which writes `census.tsv` and `census-server-chunks.tsv`
  - Done-when 4's document, `docs/re/m1-script-census.md`

- [ ] **Step 1: Branch, and write the failing tests**, `crates/fenji-lua/tests/census.rs`:

```bash
git switch dev && git pull --ff-only && git switch -c feat/lua-<issue>-census
```

```rust
mod common;

use fenji_lua::{census, parse, Counts, Dialect, Instr, Op};

fn counts(src: &str) -> Counts {
    census(&parse(&common::luac(src, true), Dialect::Stock53).unwrap())
}

#[test]
fn server_and_client_blocks_are_counted() {
    let c = counts("local x = 0\nif IsServer() then x = x + 1 x = x * 2 end\nif IsClient() then x = 7 end\nreturn x");
    assert_eq!((c.chunks, c.functions, c.reads_is_server, c.reads_is_client), (1, 1, 1, 1));
    assert_eq!((c.server_only, c.client_only), (2, 1), "{c:?}");
}

#[test]
fn negated_guards_swap_sides() {
    let plain = counts("local x = 0 if IsServer() then x = 1 end return x");
    let negated = counts("local x = 0 if not IsServer() then x = 1 end return x");
    assert_eq!((plain.server_only, plain.client_only), (1, 0));
    assert_eq!((negated.server_only, negated.client_only), (0, 1));
}

#[test]
fn an_early_return_guards_the_rest_of_the_function() {
    // `if not IsServer() then return end`: everything after it runs only on the server.
    let c = counts("local x = 0 if not IsServer() then return end x = 1 x = 2 return x");
    assert_eq!(c.client_only, 1, "the RETURN itself");
    assert!(c.server_only >= 3, "{c:?}");
}

#[test]
fn nested_functions_are_visited_and_unguarded_code_is_neither() {
    let c = counts("local function f() if IsServer() then return 1 end end local function g() return 2 end return f, g");
    assert_eq!((c.functions, c.reads_is_server), (3, 1));
    let plain = counts("local function g() return 2 end return g");
    assert_eq!((plain.server_only, plain.client_only), (0, 0));
}

#[test]
fn the_clients_global_reads_count_too() {
    let mut chunk = parse(&common::luac("if IsServer() then print(1) end", true), Dialect::Stock53).unwrap();
    for w in &mut chunk.main.code {
        let i = Instr(*w);
        if i.op() == Some(Op::GetTabUp) {
            *w = i.with_op(Op::TryTabUp).0;
        }
    }
    let c = census(&chunk);
    assert_eq!(c.reads_is_server, 1);
    assert!(c.server_only > 0);
}
```

Run: `cargo test -p fenji-lua --test census`
Expected: compile error (`fenji_lua::census` not found).

- [ ] **Step 2: Implement `crates/fenji-lua/src/census.rs`.** Add `mod census;` and `pub use census::{census, Counts};` to `lib.rs`.

```rust
//! Server-side logic in the client's scripts, measured from the bytecode (M1 spec §4, W6). The
//! measure is syntactic. It finds `if IsServer() then … end` (and IsClient, and `not` of either):
//! a read of the global, `CALL A 1 2`, `TEST A k` and a forward `JMP`. The instructions the JMP
//! skips run only when the test passes. When that block is a lone RETURN (`if not IsServer() then
//! return end`), the rest of the function runs only in the opposite case.
use std::collections::BTreeSet;
use std::ops::{AddAssign, Range};

use crate::{is_k, k_index, Chunk, Constant, Function, Instr, Op};

/// Counts for one chunk, or summed over many.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub chunks: u64,
    pub functions: u64,
    pub instructions: u64,
    /// Functions that read the global `IsServer` or `IsClient`.
    pub reads_is_server: u64,
    pub reads_is_client: u64,
    /// Instructions that run only on the server: under `IsServer()` true or `IsClient()` false.
    pub server_only: u64,
    /// Instructions that run only on the client: under `IsClient()` true or `IsServer()` false.
    pub client_only: u64,
}

impl AddAssign for Counts {
    fn add_assign(&mut self, o: Self) {
        self.chunks += o.chunks;
        self.functions += o.functions;
        self.instructions += o.instructions;
        self.reads_is_server += o.reads_is_server;
        self.reads_is_client += o.reads_is_client;
        self.server_only += o.server_only;
        self.client_only += o.client_only;
    }
}

/// The counts for one chunk.
pub fn census(chunk: &Chunk) -> Counts {
    let mut c = Counts { chunks: 1, ..Counts::default() };
    visit(&chunk.main, &mut c);
    c
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Guard {
    IsServer,
    IsClient,
}

fn visit(f: &Function, c: &mut Counts) {
    c.functions += 1;
    c.instructions += f.code.len() as u64;
    let (mut server, mut client) = (BTreeSet::new(), BTreeSet::new());
    let (mut reads_server, mut reads_client) = (false, false);
    for pc in 0..f.code.len() {
        let Some(guard) = global_read(f, pc) else { continue };
        match guard {
            Guard::IsServer => reads_server = true,
            Guard::IsClient => reads_client = true,
        }
        if let Some((block, when_true)) = guarded_block(f, pc) {
            let on_server = (guard == Guard::IsServer) == when_true;
            let early_return = block.len() == 1 && Instr(f.code[block.start]).op() == Some(Op::Return);
            let rest = block.end..f.code.len();
            let (block_side, rest_side) = if on_server { (&mut server, &mut client) } else { (&mut client, &mut server) };
            block_side.extend(block);
            if early_return {
                rest_side.extend(rest);
            }
        }
    }
    c.reads_is_server += u64::from(reads_server);
    c.reads_is_client += u64::from(reads_client);
    c.server_only += server.len() as u64;
    c.client_only += client.len() as u64;
    for p in &f.protos {
        visit(p, c);
    }
}

/// `R(A) := Up[B][K(C)]` where K(C) is the string "IsServer" or "IsClient". That is how a global
/// read compiles, as GETTABUP or the client's TRYTABUP, TRYLIBUP or XGETTABUP.
fn global_read(f: &Function, pc: usize) -> Option<Guard> {
    let i = Instr(f.code[pc]);
    if !matches!(i.op()?, Op::GetTabUp | Op::TryTabUp | Op::TryLibUp | Op::XGetTabUp) || !is_k(i.c()) {
        return None;
    }
    match f.constants.get(k_index(i.c()) as usize)? {
        Constant::ShortString(s) if s == b"IsServer" => Some(Guard::IsServer),
        Constant::ShortString(s) if s == b"IsClient" => Some(Guard::IsClient),
        _ => None,
    }
}

/// After the read at `pc`: `CALL A 1 2`, `TEST A k`, `JMP 0 +n`. The n instructions after the
/// JMP run only when the result is truthy (k = 0) or falsy (k = 1). Returns them, and which case.
fn guarded_block(f: &Function, pc: usize) -> Option<(Range<usize>, bool)> {
    let a = Instr(f.code[pc]).a();
    let at = |n: usize| f.code.get(pc + n).map(|&w| Instr(w));
    let (call, test, jmp) = (at(1)?, at(2)?, at(3)?);
    let shape = call.op() == Some(Op::Call)
        && call.a() == a
        && call.b() == 1
        && call.c() == 2
        && test.op() == Some(Op::Test)
        && test.a() == a
        && jmp.op() == Some(Op::Jmp)
        && jmp.sbx() > 0;
    if !shape {
        return None;
    }
    let start = pc + 4;
    let end = (start + jmp.sbx() as usize).min(f.code.len());
    Some((start..end, test.c() == 0))
}
```

Run: `cargo fmt --all && cargo test -p fenji-lua --test census`
Expected: 5 pass. If `server_and_client_blocks_are_counted` reports different exact numbers, read the disassembly of that source with `disassemble`, and fix the test's expectation only if the listing shows the codegen differs from the shape assumed here (`ADD`, `MUL` for the server block; one `LOADK` for the client block). The shape matcher stays as written.

- [ ] **Step 3: Add the `census` command.** Append to `tools/luatool/src/commands.rs`:

```rust
fn census_row(area: &str, c: &fenji_lua::Counts) -> String {
    format!(
        "{area}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        c.chunks, c.functions, c.instructions, c.reads_is_server, c.reads_is_client, c.server_only, c.client_only
    )
}

/// `census`: the server-logic measurement over every decodable Lua chunk. Per-area counts go to
/// `census.tsv`. Chunks under a `server/` directory, or with at least 90 % server-only
/// instructions, go to `census-server-chunks.tsv`.
pub fn census(fs: &ExeFs, prefix: &str, out: &mut Out) -> Result<Summary> {
    let mut s = Summary::default();
    let mut areas: BTreeMap<String, fenji_lua::Counts> = BTreeMap::new();
    let mut server_chunks = String::from("path\treason\tinstructions\tserver_only\n");
    for e in selected(fs, prefix) {
        match decode(fs, e) {
            Decoded::Lua(bytes, dialect) => {
                s.lua += 1;
                match fenji_lua::parse(&bytes, dialect) {
                    Ok(chunk) => {
                        let c = fenji_lua::census(&chunk);
                        *areas.entry(area(&e.path)).or_default() += c;
                        let in_server_dir = e.path.split('/').any(|d| d == "server");
                        let mostly_server = c.instructions > 0 && c.server_only * 10 >= c.instructions * 9;
                        if in_server_dir || mostly_server {
                            let why = if in_server_dir { "server/ directory" } else { "at least 90% server-only" };
                            server_chunks.push_str(&format!("{}\t{why}\t{}\t{}\n", e.path, c.instructions, c.server_only));
                        }
                    }
                    Err(err) => {
                        s.bad_chunks += 1;
                        s.fail(&e.path, err);
                    }
                }
            }
            Decoded::Other(_) => s.other += 1,
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&e.path, why);
            }
        }
    }
    let mut tsv = String::from("area\tchunks\tfunctions\tinstructions\treads_is_server\treads_is_client\tserver_only\tclient_only\n");
    let mut total = fenji_lua::Counts::default();
    for (a, c) in &areas {
        total += *c;
        tsv.push_str(&census_row(a, c));
    }
    tsv.push_str(&census_row("(all)", &total));
    std::fs::write(out.root().join("census.tsv"), tsv)?;
    std::fs::write(out.root().join("census-server-chunks.tsv"), server_chunks)?;
    Ok(s)
}
```

In `main.rs`, add the variant:

```rust
    /// Measure server-side logic: census.tsv and census-server-chunks.tsv under OUT (inside re/)
    Census {
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        out: PathBuf,
    },
```

Add `| Command::Census { out, .. }` to the `out` match. Before `unreachable!`, add the arm `(Command::Census { prefix, .. }, Some(o)) => ("census", commands::census(&fs, prefix, o)?),`.

Add to `tools/luatool/tests/cli.rs`:

```rust
#[test]
fn cli_census_writes_its_tables() {
    let (repo, exe) = setup();
    let ok = luatool(repo.path(), &exe).args(["census", "--out", "re/c"]).status().unwrap();
    assert!(ok.success());
    let tsv = std::fs::read_to_string(repo.path().join("re/c/census.tsv")).unwrap();
    let login = tsv.lines().find(|l| l.starts_with("addons/Login\t")).expect("an addons/Login row");
    let cols: Vec<&str> = login.split('\t').collect();
    assert_eq!((cols[1], cols[4]), ("1", "1"), "one chunk, one function reading IsServer: {login}");
    assert!(tsv.lines().any(|l| l.starts_with("(all)\t")));
}
```

Run: `cargo fmt --all && cargo test -p fenji-luatool`
Expected: 5 `cli.rs` tests pass.

- [ ] **Step 4: Measure the pinned client** (local), and write `docs/re/m1-script-census.md`:

```bash
cargo build --release -p fenji-luatool
FENJI_CLIENT_PATH=~/fenji-client target/release/luatool census --out re/m1/census
column -t -s$'\t' re/m1/census/census.tsv | head -60
wc -l re/m1/census/census-server-chunks.tsv
```

The doc holds tables and prose only; numbers are fine, script text is not. Sections:
- `# M1: how much server-side logic the client's scripts carry`, with a summary of three sentences.
- `## Method`:
  - the shape the measure finds, and its blind spots: other shapes such as `local s = IsServer()`, guards inside expressions, and runtime-only dispatch;
  - that it counts instructions, not behaviour;
  - the evidence keys `re/m1/census/census.tsv` and `census-server-chunks.tsv`.
- `## Results by area`: `census.tsv`'s numbers per area, with shares of `server_only` and `client_only`, plus the `(all)` row.
- `## Server-side areas`: the `server/` directories and the mostly-server chunks, as counts per area, with a short list of the largest.
- `## What this means for M5`: Inferred. Which areas look like scripts the server could run (much `IsServer` logic, as in `combat/`), and which hold little; this feeds parent spec §6.4's hybrid rule.
- `## Unknown`: what the measure can't show.

Add to `docs/README.md`:

```markdown
| [`re/m1-script-census.md`](re/m1-script-census.md) | M1: how much server-side logic the client's embedded scripts carry. |
```

- [ ] **Step 5: Gate, commit and merge.**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && scripts/check-forbidden-files.sh`

```bash
git add crates/fenji-lua tools/luatool docs/re/m1-script-census.md docs/README.md
git commit -m "feat(lua): measure server-side logic in the client's scripts" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

Wait for CI, then squash-merge.

---

### Task 12: Archive structure without decompression, and the `archive` tool

**Files:**
- Create:
  - `crates/fenji-data/src/archive.rs`
  - `tools/archive/Cargo.toml`
  - `tools/archive/src/main.rs`
  - `tools/archive/tests/cli.rs`
  - `docs/re/m1-archives.md`
- Modify:
  - `crates/fenji-data/src/lib.rs`
  - `crates/fenji-data/src/error.rs`
  - `Cargo.toml` (members)
  - `Cargo.lock`
  - `docs/README.md`

**Interfaces:**
- Consumes:
  - Task 3's `BlobHeader`, `decode_blob`, `BuiltinCodecs` and `re_dir`;
  - spike-0 6.1–6.9.
- Produces:
  - `fenji_data::archive::{PART_MAGIC, RECORDS, RECORD_LEN, TABLE_START, DATA_START, Record, PartHeader, parse_part_header(&[u8]) -> Result<PartHeader>, read_part_header(&Path) -> Result<PartHeader>, read_blob_header(&mut File, u64) -> Result<BlobHeader>}`
  - the binary `archive`, with subcommands `info`, `list --part N`, `extract --part N --stored-only --out DIR` and `index`, and a global `--client DIR` (default `$FENJI_CLIENT_PATH`)
  - `docs/re/m1-archives.md`

- [ ] **Step 1: Branch, and confirm the record layout in Ghidra** (RE: clean session only).

```bash
git switch dev && git pull --ff-only && git switch -c feat/data-<issue>-archive
```

In planning, two records of `data000` read as `[20-byte key][u32 offset][u32 size][u32 id]` (Inferred): offset `0x2000C` with size `0x59E6`, then offset `0x259F2` = `0x2000C + 0x59E6`. Confirm or correct this from the engine's own reader:
- `search_instructions` or `search_byte_patterns` for the magic as an immediate: `0x0053465A` ("ZFS\0") or `0x5D58495B` ("[IX]");
- `get_xrefs_to` → `force_decompile` the function that opens a part;
- record how it reads the record table: field offsets and sizes, what the key is (a hash of the path? which algorithm?), and whether files span parts. Spike-0 6.3 found the records tile only 10 of 100 parts.
- Also record whether a stored (codec 0) blob has a block list. Planning reads it as having none, so its data starts at header + 32.

Note the findings with addresses in `re/m1/archive-notes.md` (local). If the layout differs, change `Record` and `parse_part_header` below to match before writing the tests.

- [ ] **Step 2: Write the failing tests.** Add `#[error("archive: {0}")] BadArchive(String),` to `fenji_data::Error`. Create `crates/fenji-data/src/archive.rs` with only this test module at the bottom, and add `pub mod archive;` to `lib.rs`:

```rust
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{test_util::lz4_blob, Codec};

    /// A stored blob: a 32-byte header with codec 0 and no block list, then the raw bytes.
    pub(crate) fn stored_blob(content: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(content.len() as u64).to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&((FIXED_LEN + content.len()) as u64).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes()); // codec 0: stored
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(content);
        b
    }

    /// A part with two records: a stored Bink-like file, and a codec-4 blob (an LZ4 blob relabelled).
    pub(crate) fn part() -> Vec<u8> {
        let first = stored_blob(b"KB2j fake video bytes");
        let mut second = lz4_blob(b"abcabcabcabc", 0);
        second[28] = 4;
        let mut p = PART_MAGIC.to_vec();
        p.extend_from_slice(&0u32.to_le_bytes());
        let mut table = vec![0u8; RECORDS * RECORD_LEN];
        let mut offset = DATA_START as u32;
        for (slot, blob) in [(0usize, &first), (5, &second)] {
            let r = &mut table[slot * RECORD_LEN..(slot + 1) * RECORD_LEN];
            r[..20].copy_from_slice(&[slot as u8 + 1; 20]);
            r[20..24].copy_from_slice(&offset.to_le_bytes());
            r[24..28].copy_from_slice(&(blob.len() as u32).to_le_bytes());
            r[28..32].copy_from_slice(&(0x0100_e2dc + slot as u32).to_le_bytes());
            offset += blob.len() as u32;
        }
        p.extend_from_slice(&table);
        p.extend_from_slice(&first);
        p.extend_from_slice(&second);
        p
    }

    #[test]
    fn parses_the_record_table() {
        let h = parse_part_header(&part()).unwrap();
        assert_eq!(h.records.len(), 2);
        assert_eq!((h.records[0].slot, h.records[0].offset), (0, DATA_START as u32));
        assert_eq!((h.records[1].slot, h.records[1].id), (5, 0x0100_e2e1));
    }

    #[test]
    fn reads_blob_headers_at_record_offsets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data000");
        std::fs::write(&path, part()).unwrap();
        let h = read_part_header(&path).unwrap();
        let mut f = File::open(&path).unwrap();
        let first = read_blob_header(&mut f, u64::from(h.records[0].offset)).unwrap();
        let second = read_blob_header(&mut f, u64::from(h.records[1].offset)).unwrap();
        assert_eq!((first.codec, first.stored_size), (Codec::Stored, u64::from(h.records[0].size)));
        assert_eq!(second.codec, Codec::Oodle);
    }

    #[test]
    fn refuses_what_isnt_a_part() {
        assert!(matches!(parse_part_header(b"ZFS\0[IX]"), Err(Error::BadArchive(_))));
        assert!(matches!(parse_part_header(&[0u8; 0x2000C]), Err(Error::BadArchive(_))));
    }
}
```

Run: `cargo test -p fenji-data archive`
Expected: compile errors (`parse_part_header` and others not found).

- [ ] **Step 3: Implement `archive.rs`**, above the tests, with the layout confirmed in Step 1:

```rust
//! The data archives' structure without decompression (M1 spec §5): each `data0NN` part's header
//! and record table, and the blob headers that records point at (docs/re/m1-archives.md).
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::{BlobHeader, Error, Result, FIXED_LEN};

pub const PART_MAGIC: &[u8; 8] = b"ZFS\0[IX]";
/// Records per part (spike-0 6.2).
pub const RECORDS: usize = 4096;
pub const RECORD_LEN: usize = 32;
pub const TABLE_START: usize = 12;
/// The first byte after the record table (0x2000C).
pub const DATA_START: u64 = (TABLE_START + RECORDS * RECORD_LEN) as u64;
/// More blocks than this means a corrupt header; no archive file comes near it.
const MAX_BLOCKS: u64 = 1 << 20;

/// One record of a part's table (field meanings: docs/re/m1-archives.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub slot: u16,
    pub key: [u8; 20],
    pub offset: u32,
    pub size: u32,
    pub id: u32,
}

impl Record {
    pub fn is_empty(&self) -> bool {
        self.size == 0 && self.key == [0; 20]
    }
}

/// A part's 4-byte header field and its non-empty records, in slot order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartHeader {
    pub field: u32,
    pub records: Vec<Record>,
}

/// Parses the first `DATA_START` bytes of a part.
pub fn parse_part_header(bytes: &[u8]) -> Result<PartHeader> {
    if bytes.get(..8) != Some(PART_MAGIC.as_slice()) {
        return Err(Error::BadArchive("no ZFS\\0[IX] magic".into()));
    }
    let table = bytes
        .get(TABLE_START..DATA_START as usize)
        .ok_or_else(|| Error::BadArchive("record table truncated".into()))?;
    let field = u32::from_le_bytes(bytes[8..12].try_into().expect("4 bytes"));
    let word = |r: &[u8], o: usize| u32::from_le_bytes(r[o..o + 4].try_into().expect("4 bytes"));
    let records = table
        .chunks_exact(RECORD_LEN)
        .enumerate()
        .map(|(slot, r)| Record {
            slot: slot as u16,
            key: r[..20].try_into().expect("20 bytes"),
            offset: word(r, 20),
            size: word(r, 24),
            id: word(r, 28),
        })
        .filter(|r| !r.is_empty())
        .collect();
    Ok(PartHeader { field, records })
}

/// Reads a part's header: its first `DATA_START` bytes, never the 1 GiB behind them.
pub fn read_part_header(path: &Path) -> Result<PartHeader> {
    let mut buf = vec![0; DATA_START as usize];
    File::open(path)?.read_exact(&mut buf)?;
    parse_part_header(&buf)
}

/// The blob header at `offset`: the fixed part, plus the block list for compressed codecs.
pub fn read_blob_header(file: &mut File, offset: u64) -> Result<BlobHeader> {
    let mut head = vec![0; FIXED_LEN];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut head)?;
    let raw = u64::from_le_bytes(head[0..8].try_into().expect("8 bytes"));
    let block = u32::from_le_bytes(head[24..28].try_into().expect("4 bytes"));
    let codec = u16::from_le_bytes([head[28], head[29]]);
    let blocks = if codec == 0 || raw == 0 || block == 0 { 0 } else { raw.div_ceil(u64::from(block)) };
    if blocks > MAX_BLOCKS {
        return Err(Error::BadBlob { at: offset, reason: format!("{blocks} blocks") });
    }
    head.resize(FIXED_LEN + 4 * blocks as usize, 0);
    file.read_exact(&mut head[FIXED_LEN..])?;
    BlobHeader::parse(&head, offset)
}
```

Run: `cargo test -p fenji-data`
Expected: all pass, including the 3 archive tests.

- [ ] **Step 4: The `archive` tool.** Add `"tools/archive"` to the root `members`. `tools/archive/Cargo.toml`:

```toml
[package]
name = "fenji-archive"
description = "Reads the SoLO data archives' structure (parts, records, blob headers, _index) without decompressing Oodle data; writes only under re/"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[[bin]]
name = "archive"
path = "src/main.rs"

[dependencies]
anyhow.workspace = true
clap.workspace = true
fenji-data.workspace = true

[dev-dependencies]
fenji-data = { workspace = true, features = ["test-util"] }
tempfile.workspace = true

[lints]
workspace = true
```

`tools/archive/src/main.rs`:

```rust
//! archive: the SoLO data archives' structure without decompression (M1 spec §5). Oodle (codec 4)
//! waits on the M1 decision gate; only stored blobs can be extracted. Writes only under re/.
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use fenji_data::archive::{read_blob_header, read_part_header};
use fenji_data::{decode_blob, BuiltinCodecs, Codec};

/// The SoLO data archives, read without decompression. Writes only under re/.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The client copy (default: $FENJI_CLIENT_PATH)
    #[arg(long, global = true)]
    client: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Per part: non-empty records, blob codecs, and records that disagree with their blob
    Info,
    /// One part's records as TSV: slot, key, offset, size, id, codec, raw size, stored size
    List {
        #[arg(long)]
        part: u32,
    },
    /// Write one part's stored (uncompressed) blobs under OUT (inside re/)
    Extract {
        #[arg(long)]
        part: u32,
        #[arg(long)]
        stored_only: bool,
        #[arg(long)]
        out: PathBuf,
    },
    /// The _index blob header
    Index,
}

fn part_path(client: &Path, n: u32) -> PathBuf {
    client.join("data").join(format!("data{n:03}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = match cli.client {
        Some(c) => c,
        None => PathBuf::from(std::env::var_os("FENJI_CLIENT_PATH").context("pass --client or set FENJI_CLIENT_PATH")?),
    };
    match cli.command {
        Command::Info => info(&client),
        Command::List { part } => list(&client, part),
        Command::Extract { part, stored_only, out } => {
            if !stored_only {
                bail!("only --stored-only is available until the Oodle decision gate is decided");
            }
            extract_stored(&client, part, &out)
        }
        Command::Index => {
            let h = read_blob_header(&mut File::open(client.join("data").join("_index"))?, 0)?;
            println!(
                "raw {} stored {} timestamp {} block size {} codec {} ({}) check {:#06x} blocks {}",
                h.raw_size, h.stored_size, h.timestamp, h.block_size, h.codec.as_u16(), h.codec.name(), h.check, h.block_sizes.len()
            );
            Ok(())
        }
    }
}

fn info(client: &Path) -> Result<()> {
    println!("part\trecords\tstored\tlz4\toodle\tother\trecord_size_mismatch\tbad_header");
    for n in 0..1000 {
        let path = part_path(client, n);
        if !path.exists() {
            break;
        }
        let h = read_part_header(&path)?;
        let mut f = File::open(&path)?;
        let mut codecs: BTreeMap<&str, u64> = BTreeMap::new();
        let (mut mismatch, mut bad) = (0u64, 0u64);
        for r in &h.records {
            match read_blob_header(&mut f, u64::from(r.offset)) {
                Ok(b) => {
                    *codecs.entry(match b.codec {
                        Codec::Stored => "stored",
                        Codec::Lz4 => "lz4",
                        Codec::Oodle => "oodle",
                        Codec::Other(_) => "other",
                    }).or_default() += 1;
                    if b.stored_size != u64::from(r.size) {
                        mismatch += 1;
                    }
                }
                Err(_) => bad += 1,
            }
        }
        let get = |k: &str| codecs.get(k).copied().unwrap_or(0);
        println!("{n:03}\t{}\t{}\t{}\t{}\t{}\t{mismatch}\t{bad}", h.records.len(), get("stored"), get("lz4"), get("oodle"), get("other"));
    }
    Ok(())
}

fn list(client: &Path, part: u32) -> Result<()> {
    let path = part_path(client, part);
    let h = read_part_header(&path)?;
    let mut f = File::open(&path)?;
    println!("slot\tkey\toffset\tsize\tid\tcodec\traw\tstored");
    for r in &h.records {
        let (codec, raw, stored) = match read_blob_header(&mut f, u64::from(r.offset)) {
            Ok(b) => (b.codec.name().to_string(), b.raw_size.to_string(), b.stored_size.to_string()),
            Err(e) => (format!("bad header: {e}"), "?".into(), "?".into()),
        };
        println!("{}\t{}\t{:#x}\t{}\t{:#010x}\t{codec}\t{raw}\t{stored}", r.slot, hex(&r.key), r.offset, r.size, r.id);
    }
    Ok(())
}

fn extract_stored(client: &Path, part: u32, out: &Path) -> Result<()> {
    let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
    let root = fenji_data::re_dir::resolve_out_dir(&repo, out)?.join(format!("data{part:03}"));
    std::fs::create_dir_all(&root)?;
    let path = part_path(client, part);
    let h = read_part_header(&path)?;
    let mut f = File::open(&path)?;
    let (mut written, mut skipped) = (0u64, 0u64);
    for r in &h.records {
        let Ok(b) = read_blob_header(&mut f, u64::from(r.offset)) else {
            skipped += 1;
            continue;
        };
        if b.codec != Codec::Stored {
            skipped += 1;
            continue;
        }
        let mut blob = vec![0; b.stored_size as usize];
        f.seek(SeekFrom::Start(u64::from(r.offset)))?;
        f.read_exact(&mut blob)?;
        let content = decode_blob(&blob, u64::from(r.offset), &BuiltinCodecs)?;
        let ext = match content.get(..4) {
            Some(b"KB2j") => "bk2",
            Some(b"RIFF") => "riff",
            _ => "bin",
        };
        std::fs::write(root.join(format!("{}.{ext}", r.slot)), content)?;
        written += 1;
    }
    eprintln!("data{part:03}: {written} stored blobs written to {}, {skipped} compressed or unreadable skipped", root.display());
    Ok(())
}
```

`tools/archive/tests/cli.rs` builds a synthetic client in a temp checkout. The part bytes repeat the `part()` helper from Step 2, because unit-test helpers aren't visible to integration tests:

```rust
use std::process::Command;

fn stored_blob(content: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&(content.len() as u64).to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes());
    b.extend_from_slice(&((32 + content.len()) as u64).to_le_bytes());
    b.extend_from_slice(&[0; 8]); // block size 0, codec 0 (stored), check 0
    b.extend_from_slice(content);
    b
}

fn setup() -> tempfile::TempDir {
    use fenji_data::archive::{DATA_START, PART_MAGIC, RECORDS, RECORD_LEN};
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    std::fs::create_dir(repo.path().join("re")).unwrap();
    let first = stored_blob(b"KB2j fake video bytes");
    let mut second = fenji_data::test_util::lz4_blob(b"abcabcabcabc", 0);
    second[28] = 4; // codec 4: Oodle
    let mut p = PART_MAGIC.to_vec();
    p.extend_from_slice(&0u32.to_le_bytes());
    let mut table = vec![0u8; RECORDS * RECORD_LEN];
    let mut offset = DATA_START as u32;
    for (slot, blob) in [(0usize, &first), (5, &second)] {
        let r = &mut table[slot * RECORD_LEN..(slot + 1) * RECORD_LEN];
        r[..20].copy_from_slice(&[slot as u8 + 1; 20]);
        r[20..24].copy_from_slice(&offset.to_le_bytes());
        r[24..28].copy_from_slice(&(blob.len() as u32).to_le_bytes());
        offset += blob.len() as u32;
    }
    p.extend_from_slice(&table);
    p.extend_from_slice(&first);
    p.extend_from_slice(&second);
    std::fs::create_dir_all(repo.path().join("client/data")).unwrap();
    std::fs::write(repo.path().join("client/data/data000"), p).unwrap();
    repo
}

fn archive(repo: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_archive"));
    c.current_dir(repo).arg("--client").arg(repo.join("client"));
    c
}

#[test]
fn info_and_list_show_codecs() {
    let repo = setup();
    let out = archive(repo.path()).arg("info").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("000\t2\t1\t0\t1\t0\t0\t0"));
    let out = archive(repo.path()).args(["list", "--part", "0"]).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("\tstored\t") && text.contains("\toodle\t"), "{text}");
}

#[test]
fn extract_writes_only_stored_blobs_under_re() {
    let repo = setup();
    let ok = archive(repo.path()).args(["extract", "--part", "0", "--stored-only", "--out", "re/a"]).status().unwrap();
    assert!(ok.success());
    assert_eq!(std::fs::read(repo.path().join("re/a/data000/0.bk2")).unwrap(), b"KB2j fake video bytes");
    assert!(!repo.path().join("re/a/data000/5.bin").exists());
    let refused = archive(repo.path()).args(["extract", "--part", "0", "--stored-only", "--out", "elsewhere"]).status().unwrap();
    assert!(!refused.success());
}
```

Run: `cargo fmt --all && cargo test -p fenji-archive`
Expected: 2 pass.

- [ ] **Step 5: Measure the real archives** (local; reads headers only), and write `docs/re/m1-archives.md`.

```bash
cargo build --release -p fenji-archive
FENJI_CLIENT_PATH=~/fenji-client target/release/archive info > re/m1/archive-info.tsv
FENJI_CLIENT_PATH=~/fenji-client target/release/archive index
FENJI_CLIENT_PATH=~/fenji-client target/release/archive extract --part 0 --stored-only --out re/m1/archive-stored
```

`docs/re/m1-archives.md` holds prose and tables, every finding tagged, with evidence `re/m1/archive-info.tsv`, `re/m1/archive-notes.md` and Ghidra addresses. Sections:
- `# M1: the data archives without decompression`
- `## Part layout`: magic, field, record table (Step 1's confirmed fields), and data start
- `## Records and blobs`: codec counts across all parts; whether record sizes equal blob stored sizes; the tiling question from spike-0 6.3, answered or Unknown
- `## _index`: header fields, its 49 blocks, and that its content needs codec 4
- `## Stored files`: what kinds of file are stored uncompressed (by magic)
- `## What Oodle would unlock`: path names (`_index`), tables, and possibly complete copies of the 79 scripts truncated in the exe. That copies exist is Unknown until `_index` is readable.

Add to `docs/README.md`:

```markdown
| [`re/m1-archives.md`](re/m1-archives.md) | M1: the data archives' structure, read without decompressing Oodle data. |
```

- [ ] **Step 6: Gate, commit and merge.**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo deny check && scripts/check-forbidden-files.sh`

```bash
git add Cargo.toml Cargo.lock crates/fenji-data tools/archive docs/re/m1-archives.md docs/README.md
git commit -m "feat(data): read the archives' structure without decompression" -m "Closes #<issue>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014j8e76oH1TnqWYHP2fuyy1"
git push -u origin HEAD && gh pr create --base dev --fill
```

Wait for CI, then squash-merge.

---

### Task 13: Oodle decision follow-through

**Files:** depend on the owner's decision, which will be on Task 2's gate issue.

**Interfaces:**
- Consumes the decision, Task 3's `Decompressor` trait (codec 4 plugs in behind it) and Task 12's archive reader.
- Produces one of three things:
  - a plan amendment, approved by the owner, plus its tasks;
  - the owner's ruling that table dumps leave M1, recorded in the spec;
  - for option (d), a follow-up issue.

- [ ] **Step 1: Read the decision** on the gate issue. Until it is filled in, this task waits; continue with Task 14's Steps 1–2.

- [ ] **Step 2: Act on it.**
  - **(a) clean-room decoder, (b) external decoder, or (c) the client's DLL in a sandboxed helper.** Write a plan amendment as a new section at the end of this plan, `## Amendment: archive decompression (gate #<n>, option <x>)`, with tasks in this plan's format: files, interfaces, TDD steps with code, commands. It covers:
    - the codec-4 `Decompressor` implementation, or an external runner behind the same trait;
    - decoding `_index` and reading its path → record mapping (RE of its format);
    - reading the `.tab` table format (RE);
    - `archive dump` for items, skills, monsters, the XP curve and maps, with output under `re/`;
    - for (c), the helper's sandbox (`isolated-run.sh`, no network) and how the DLL is loaded from the owner's copy only.

    Ask the owner to approve the amendment, then execute it inline like the tasks above.
  - **(d) defer.** Add a short "M1 update" note to parent spec §10 (M1 row), saying that table dumps moved out of M1 by the owner's ruling (gate #<n>), and open a `Task` issue in milestone M2 for them. The same applies if the owner rules a slow (a), (b) or (c) route out of M1's Done-when.

- [ ] **Step 3:** Close this task's issue, linking to the amendment or the ruling.

---

### Task 14: M1 close-out: records, spec notes, final review, v0.2.0

**Files:**
- Create: `docs/re/m1-summary.md`
- Modify:
  - `docs/superpowers/specs/2026-10-11-fenji-design.md` (§10, §12)
  - `docs/superpowers/specs/2026-10-12-m1-data-layer-design.md` (status line)
  - `docs/README.md`
  - `CLAUDE.md` (Status, at release)
- Generated: `CHANGELOG.md` (at release)

**Interfaces:**
- Consumes every earlier task's documents.
- Produces:
  - M1's Done-when 7 records;
  - a clean final review;
  - with the owner's go-ahead, the `v0.2.0` release.

- [ ] **Step 1: Write `docs/re/m1-summary.md`.** Branch `docs/re-<issue>-m1-close`.
  - **Spike-0 questions.** One table row per open M1 question from spike-0: 6.4 (record format), 6.7 (encryption), 6.9 (`_index` format), 6.16 (the header field: answered, `size_t`), 6.19 (opcode numbering: answered, stock 0–46), 6.21 (server-side logic: the census), 8.19 (the German UI) and 8.37 (the input that makes the client connect). For each: the answer, or "carried to M2/M3", and the doc that holds it.
  - **Deviations.** One row per deviation D1–D4, with how it ended.
  - **The M1 Done-when list.** Each item, its status and its evidence.

- [ ] **Step 2: Add "M1 update" notes to the parent spec.**
  - In §10, under the M1 row, add a note on what M1 delivered and any of the owner's rulings (Task 13).
  - In §12, add an "Answer (M1)" column for the rows M1 answers ("archive and index format", "how much server-side logic"), linking `m1-archives.md`, `m1-script-census.md` and `login.md`. Mark the rest "—".
  - Set the M1 spec's status line to `implemented in M1 (v0.2.0); see docs/re/m1-summary.md`.
  - Add the `docs/README.md` row: ``| [`re/m1-summary.md`](re/m1-summary.md) | M1 summary: spike-0's questions answered, deviations, Done-when status. |``
  - Commit `docs(re): summarise M1 and update the spec`, open a PR, and wait for CI. **Hold the merge until Step 3's review is clean.**

- [ ] **Step 3: Final whole-branch review.**
  - Run `git log --oneline origin/main..origin/dev`, then build the package: `git diff origin/main origin/dev > re/m1/final-review.diff` (local).
  - Dispatch one fresh reviewer (`general-purpose`, `opus`), which must never have read rejected material. Its brief:
    - the M1 spec and this plan;
    - the Global Constraints and Review Focus above;
    - the diff file;
    - Done-when 1–7, checked against the evidence;
    - "triage every deferred note in `re/m1/progress.md`".
  - Make one fix wave for what it finds, then one scoped re-review. Merge the Step 2 PR when it is clean.

- [ ] **Step 4: Release v0.2.0, only after the owner says go.** Follow `docs/development.md`'s release procedure exactly, with the tag `v0.2.0` and milestone "M1 Data layer":
  - explicit staging of `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md` and `CLAUDE.md`, with the Status line saying M1 is released and M2 is next;
  - check `git diff --cached --stat` before committing;
  - the release notes name the pinned build;
  - close the release issue (Task 14's) and the milestone.

  Run it step by step on the Fedora box, checking each output. Do the release-notes check (`cat` the notes file) before `gh release create`.

Expected:
- a `v0.2.0` tag on `main`;
- a `## [0.2.0]` section in `CHANGELOG.md`;
- release notes that name the pinned build;
- milestone M1 closed.
