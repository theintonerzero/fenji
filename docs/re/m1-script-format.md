# M1: the client's embedded scripts

`gujianol.exe` carries 21,512 files in an embedded file system: 20,443 compiled Lua 5.3 chunks plus XML, `.toc`, config and C files. Each one is an LZ4-compressed blob that the engine mounts under `data/` ahead of the archives, and every one of them decodes. The chunks use a slightly changed header and three extra opcodes that cache function lookups at run time. Normalisation rewrites two of those opcodes to `GETTABUP` and removes the third, after which a stock Lua 5.3 decompiler reads the chunks.

**Method.** The pinned build 10015727, `bin64/gujianol.exe` SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678` ([`pinned-build.md`](pinned-build.md)). Standard-library Python probes read the exe and `data/_index` from disk. Ghidra was queried read-only through the shared project: no renames, comments, scripts or debugger. The normalisation trial in §4 ran the unluac decompiler on three normalised chunks. The client was not executed or modified, and nothing from it is in the repo. REA was not used, because the Ghidra project is the surface for this exe; addresses are virtual addresses in the pinned exe (image base 0x140000000). Every part of this document was written by an agent that has not read any source rejected in [`sources.md`](sources.md).

| Tag | Meaning |
|---|---|
| **Observed** | Read from the pinned build's files by a probe. Anyone with the pinned build can reproduce it. |
| **Inferred** | Our reading of observed facts, including our reading of code in Ghidra. Not confirmed at run time. |
| **Unknown** | Open. |

Evidence keys such as `RW` or `G4` resolve in the [Evidence index](#evidence-index): file keys point under `re/m1/` (git-ignored, on the owner's machines), and `G` keys are addresses in the pinned exe.

In short:
- The file system is a mount table of five directories, `data/{c,locale,core,interface,scripts}`, each a list of 24-byte file entries. Large files span several entries. Read that way, all 21,512 files decode (§1). **Observed**.
- The engine mounts the embedded directories after the archives, and a mount replaces a same-named directory, so the embedded copies win (§1). **Inferred**.
- Every file is one blob with a 32-byte header, the same header `_index` has; all embedded blobs use codec 1, raw LZ4 blocks (§2). **Observed**.
- The chunks are stock Lua 5.3 dumps, except that the header omits the `size_t` byte. Opcodes 0–46 are stock, and every chunk is stripped (§3). **Observed** and **Inferred**.
- Of the seven extra opcodes, only `TRYTABUP`, `TRYLIBUP` and `TRYSELF` occur. On first execution the VM rewrites each of them, either into its stock form or into a cached form (§4). **Inferred**.
- In the dumps, `TRYSELF` always sits right before an identical `SELF`. So normalisation maps `TRYTABUP` and `TRYLIBUP` to `GETTABUP` and removes `TRYSELF`; it is not one instruction for one instruction (§4).
- No entry is missing or cut short. The planning finding of 79 scripts truncated at 16 KiB was a misreading of multi-piece files (§5). **Observed**.

## 1. The embedded file system

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 1.1 | A mount table at 0x143197720 in `.data` holds 24-byte entries (pointers to a mount name, a directory name and the directory's first file entry) and ends at a NULL mount name. It has five entries, all with mount name `data`: `c` (first entry 0x143122FB0), `locale` (0x143123090), `core` (0x143123E10), `interface` (0x1431977B0) and `scripts` (0x143127EC0). | **Observed** | RW, G1 |
| 1.2 | A file entry is 24 bytes: a pointer to the path (a NUL-terminated string in `.rdata`), a u32 stored size, a u32 continuation flag and a pointer to a piece of the stored blob in `.rdata`. A directory's list ends at a NULL path. | **Observed** | RW, G2 |
| 1.3 | A file whose flag is nonzero continues in the following entries until one with a zero flag. Its pieces are concatenated, each 0x3FFC (16,380) bytes except the last; the path and size come from the first entry. | **Observed** (all files decode read this way); the meaning is **Inferred** from the walker | RW, G2 |
| 1.4 | 21,512 files. 87 have more than one piece, up to 31; the largest, `addons/Pinyin/Dict-zi.luac`, stores 501,865 bytes. No path occurs twice across the five directories. | **Observed** | RW |
| 1.5 | By extension: `.luac` 20,443, `.xml` 642, `.toc` 411, `.config` 7, `.c` 6, `.h` 2, `.txt` 1. | **Observed** | RS |
| 1.6 | By directory: `scripts` 18,855 (`combat/` 18,686, `emotionaction/` 89, `actions/` 48, `move_formular/` 32); `interface` 1,896 (`addons/` 1,810, `templates/` 61, `system/` 24, `blacklist.txt`); `core` 659 (`modules/` 548, `combat/` 41, `share/` 38, `client/` 31, `xobject/` 1); `locale` 94 (`common/` 70, the region folders `mysg`, `vi`, `gameforge`, `kr`, `steam` and `tw`, and `load.config`); `c` 8 (TinyCC sources, among them `_module_dll.c` and `_module_tcc.c`). | **Observed** | RS |
| 1.7 | The login addon is `interface/addons/Login/`: `Login.luac`, `Login.toc`, `Login.xml`, `Login_i18n.xml`, `License.luac`, `License.xml`, `Staff.luac`, `Staff.xml`, `UserProtocol.luac` and `UserProtocol.xml`. | **Observed** | RS |
| 1.8 | There are seven `load.config` files, all under `locale/`: one at its top and one each in `common`, `gameforge`, `kr`, `mysg`, `steam` and `vi`. | **Observed** | RS |
| 1.9 | Mounting: FUN_140690410 walks the mount table. For each entry, FUN_1400B8DE0 builds an in-memory directory and has the walker FUN_140690380 add every file through FUN_1400B8C10. FUN_1400B4170 then mounts the directory under the mount name, which gives `data/c`, `data/locale`, `data/core`, `data/interface` and `data/scripts`. | **Inferred** | G1–G4 |
| 1.10 | FUN_1400B8C10 registers each file whole. It parses the 32-byte blob header in the first piece and checks it against the registered size. It then keeps the size, the piece size (0x3FFC, passed by the walker) and every piece pointer. | **Inferred** | G2, G3 |
| 1.11 | A lookup walks from the path to the file one component at a time, through named directory nodes. Each node keeps its children in a binary search tree ordered by name. | **Inferred** | G4 |
| 1.12 | Order. FUN_14068FB60 parses `--fs=<dir>:<data index>:<asset index>` and loads the archive indexes from `<root>/<dir>` (FUN_1400BE960). Only after that does it call FUN_140690410. `<root>` is the parent of the exe's directory, which is the install root: FUN_1400B6580 returns the exe's directory, and FUN_14068FB60 cuts that back by one more level. | **Inferred** | G5 |
| 1.13 | Precedence. Mounting a node replaces and frees an existing node of the same name (FUN_1400B4170). So the five embedded directories replace any same-named archive directory under `data/`, and there is no per-file fallback to the archives inside them. | **Inferred** | G4, G5 |
| 1.14 | Without `--fs=`, no archive is mounted, `<root>/asset` is mounted as a plain directory, and the embedded directories are mounted as usual. | **Inferred** | G5 |
| 1.15 | Whether the archives hold anything under those five names, and so whether anything is hidden. The archive index is Oodle-compressed (§2). | **Unknown** | |

## 2. The blob header

| Offset | Type | Field |
|---|---|---|
| 0 | u64 | raw (decoded) size |
| 8 | u64 | timestamp |
| 16 | u64 | stored size: the whole blob, this header included |
| 24 | u32 | block size |
| 28 | u16 | codec: 0 stored, 1 LZ4, 4 Oodle |
| 30 | u16 | `check` |
| 32 | u32 × blocks | compressed size of each block; blocks = ⌈raw ÷ block size⌉ |
| 32 + 4 × blocks | | the blocks, back to back |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | Every embedded file is one blob with this header. Its stored size equals 32 + 4 × blocks + the sum of the block sizes, and equals the size in its file entry (21,512 of 21,512). | **Observed** | RW, RS |
| 2.2 | All 21,512 use codec 1. Each block is a raw LZ4 block, without a frame, that decodes to exactly the block size (the last block to the remainder). All 21,512 decode to their raw size. | **Observed** | RW |
| 2.3 | 20,965 files are a single block, with block size equal to the raw size; 539 use 128-byte blocks and 8 use 512 KiB blocks. | **Observed** | RS |
| 2.4 | `data/_index` has the same header: raw 25,533,513, stored 5,736,039 (the file size), block size 524,288, codec 4, 49 blocks. The same identity holds. | **Observed** | IX |
| 2.5 | Codec 0 means stored and codec 4 means Oodle ([`spike-0.md`](spike-0.md) 6.6). No embedded file uses either. | **Inferred** | spike-0 6.6 |
| 2.6 | The timestamp is Unix seconds. Embedded files run from 2021-11-17 10:54:27 to 2022-11-25 09:05:36 UTC, and `_index` reads 2022-11-25 09:28:58 UTC, the same date [`spike-0.md`](spike-0.md) 6.8 gave it. | **Inferred** | IX |
| 2.7 | `check` takes 15,529 distinct values over the 21,512 embedded files. What it checks. | **Unknown** | RS |

## 3. The Lua dialect

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 3.1 | Every `.luac` (20,443) decodes to a chunk whose 32-byte header is `1B 4C 75 61 53 00 19 93 0D 0A 1A 0A 04 04 08 08`, followed by `LUAC_INT` 0x5678 and `LUAC_NUM` 370.5. Stock Lua 5.3 has five size bytes there (`04 08 04 08 08`). | **Observed** | RW |
| 3.2 | After the header (and the main closure's upvalue count), the rest is the stock Lua 5.3 dump layout, with 8-byte string lengths (a size byte of 0xFF is followed by 8 bytes). All 20,443 chunks parse that way to the last byte. | **Observed** | RW |
| 3.3 | So the header lacks only the `size_t` byte, and the loader takes `size_t` as 8 bytes. This answers [`spike-0.md`](spike-0.md) 6.16. | **Inferred** | RW |
| 3.4 | All 100,465 functions are stripped: no source name, line info, local names or upvalue names. | **Observed** | RS |
| 3.5 | All 100,465 functions end in opcode 38, stock `RETURN`. | **Observed** | IX |
| 3.6 | The opcode-name table at 0x142EF9DD0 holds 54 names and a NULL: the 47 stock names in stock order, then `NOP`, `XGETTABUP`, `TRYTABUP`, `TRYLIBUP`, `XGETSELF`, `XSELF` and `TRYSELF`. | **Observed** | IX, G9 |
| 3.7 | `luaV_execute` is FUN_140136420. It masks the instruction with 0x3F, bounds it at 53 and dispatches through a 54-entry jump table at 0x14013878C (dispatch at 0x1401364DB). Cases 0 (`MOVE`), 1 (`LOADK`), 2 (`LOADKX`) and 38 (`RETURN`) do what stock 5.3 does. | **Inferred** | G6 |
| 3.8 | Opcodes 0–46 keep their stock numbers and meanings. This answers [`spike-0.md`](spike-0.md) 6.19. | **Inferred** | IX, G6, G9 |
| 3.9 | No instruction in the dumps uses an opcode above 53. Of the extra opcodes, only 49 (`TRYTABUP`, 138,107 times), 50 (`TRYLIBUP`, 46,634) and 53 (`TRYSELF`, 291,131) occur. | **Observed** | XO |

## 4. Extra opcodes

Instruction fields are stock 5.3: opcode in bits 0–5, A in 6–13, C in 14–22, B in 23–31, Bx in 14–31. "Cache" means a VM-wide table of functions, reached from `global_State` at +0x40, whose entry count sits at `global_State`+0xD4 and is capped at 0xFFFF (FUN_140136000). Each TRY opcode's lookup only sees tables marked by a flag byte at `Table`+0x0D (nonzero, without bit 0x10), indexed by short-string keys and followed through `__index` metatables, at most 2,000 levels deep. Anything else reads as nil, so the instruction falls back to its stock form (FUN_140135F70, FUN_140135E50). The VM column is **Inferred**; the dumps column is **Observed**.

| Op | Name | Operands | What the VM does (G6–G8) | In the dumps (XO) |
|---|---|---|---|---|
| 47 | `NOP` | none read | Nothing. It shares `EXTRAARG`'s empty case (0x1401364AC). When `TRYLIBUP` rewrites itself to `NOP`, it keeps the replaced fields in the operand bits. | 0 |
| 48 | `XGETTABUP` | A, Bx | R(A) := cache[Bx] (case 0x1401385B1). | 0 |
| 49 | `TRYTABUP` | A, B, C | FUN_140136090 looks up UpValue[B][RK(C)]. If the result is a function and the cache has room, it caches the function and rewrites the instruction to `XGETTABUP A idx`; otherwise it rewrites it to `GETTABUP A B C`. Either way the rewritten instruction then runs. | 138,107, in 16,925 chunks |
| 50 | `TRYLIBUP` | A, B, C | FUN_140136180 looks up UpValue[B][RK(C)]. If that is a table, the next instruction's C is a constant K, the cache has room and table[K] is a function: it caches the function, rewrites the next instruction to `XGETTABUP A idx` and itself to `NOP`, and carries on with the next instruction. Otherwise it rewrites itself to `GETTABUP A B C` and runs that. | 46,634, in 3,844 chunks |
| 51 | `XGETSELF` | A, B | R(A+1) := R(B) (case 0x140138627). | 0 |
| 52 | `XSELF` | A, Bx | R(A) := cache[Bx]: the same case as `XGETTABUP`. | 0 |
| 53 | `TRYSELF` | A, B, C | FUN_1401362E0 raises `'tryself': self is nil!` if R(B) is nil. Otherwise, if R(B)[RK(C)] is a function and the cache has room, it caches the function, rewrites itself to `XGETSELF A B C` and the next instruction to `XSELF A idx`, then runs both. Otherwise it rewrites itself to a bare `NOP` and carries on with the next instruction. | 291,131, in 19,386 chunks |

How they sit in the dumps:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.1 | Every `TRYSELF` (291,131) is immediately followed by `SELF` with the same A, B and C. Every `SELF` in the dumps (291,131) has such a `TRYSELF` in front of it. | **Observed** | XO |
| 4.2 | No jump lands on a `SELF` that follows a `TRYSELF`; 101,089 jump targets are a `TRYSELF`. | **Observed** | XO |
| 4.3 | Every `TRYLIBUP` is followed by `GETTABLE`. In 46,633 of 46,634 that instruction is `GETTABLE A A K`: the library's function lands in the register that held the library. | **Observed** | XO |
| 4.4 | The exception is `client/game_system.luac`, function 15, pc 0: `TRYLIBUP` A=1, then `GETTABLE` A=2 B=1. The VM's cached rewrite would put the function in R(1) instead of R(2). This doesn't affect normalisation, which keeps the `GETTABLE`. | **Observed**; the effect is **Inferred** | XO, G7 |
| 4.5 | `TRYTABUP`'s and `TRYLIBUP`'s C is always a constant. B is 0 in 123,724 of 138,107 `TRYTABUP` and 38,408 of 46,634 `TRYLIBUP`. Plain `GETTABUP` still occurs 54,077 times. | **Observed** | XO |
| 4.6 | No extra opcode follows an instruction that skips the next one (`EQ`, `LT`, `LE`, `TEST`, `TESTSET`, or `LOADBOOL` with C ≠ 0). | **Observed** | XO |
| 4.7 | The client's compiler emits `TRYTABUP` and `TRYLIBUP` where stock `luac` emits `GETTABUP`, and puts a `TRYSELF` in front of every `SELF`. The VM uses them to cache functions the first time they run. `NOP`, `XGETTABUP`, `XGETSELF` and `XSELF` only appear in rewritten code at run time, which is why no dump contains them. | **Inferred** | G7, XO |

### The normalisation table

`fenji-lua`'s normaliser (M1 plan Task 7) implements exactly this table.

| Client opcode | Becomes | Why |
|---|---|---|
| 0–46 | unchanged | Stock. |
| 49 `TRYTABUP` A B C | 6 `GETTABUP` A B C | The VM's own fallback rewrite. The cached form produces the same value. |
| 50 `TRYLIBUP` A B C | 6 `GETTABUP` A B C | The same fallback. The following `GETTABLE` is kept as it is. |
| 53 `TRYSELF` A B C | removed | It is a prefix to the identical `SELF` that follows (4.1), and the stock code for `obj:m()` is that `SELF` alone. |
| 47 `NOP`, 48 `XGETTABUP`, 51 `XGETSELF`, 52 `XSELF` | unmapped: the chunk is refused | Run-time forms. The Bx of `XGETTABUP` and `XSELF` indexes a cache that exists only inside a running client. |

Removing a `TRYSELF` follows these rules:
- A `TRYSELF` at index i is removed only if instruction i+1 is `SELF` with the same A, B and C; otherwise the chunk is refused.
- Every sBx jump (`JMP`, `FORLOOP`, `FORPREP`, `TFORLOOP`) is re-targeted to its target's new index. A jump that landed on a removed `TRYSELF` lands on its `SELF`, which takes the `TRYSELF`'s place.
- A chunk is refused if a removed `TRYSELF` follows an instruction that skips or consumes the next one: `EQ`, `LT`, `LE`, `TEST`, `TESTSET`, `LOADBOOL` with C ≠ 0, `LOADKX`, `SETLIST` with C = 0, or `TFORCALL`. None does (4.6).
- Line info loses the removed entries, and local-variable ranges are re-targeted like jumps. The client's chunks have neither (3.4).

This is the one place normalisation is not one instruction for one instruction. With the planning default of `TRYSELF` → `SELF` instead, every method lookup comes out of the decompiler twice.

Trial (NT): three client chunks normalised with this table decompile with unluac 2025_12_23 without errors. The planning run, which used `TRYSELF` → `SELF`, decompiled the same chunks too, but with each method lookup duplicated. That is 80, 70 and 96 extra lines: two per removed `TRYSELF` (40, 35 and 48). **Observed**. Because the chunks are stripped, unluac writes every register as a numbered local and spells a method call as three steps: copy the object, look up the method, call it. It doesn't use the `object:method()` form.

### Decompile results

`luatool decompile` normalised every Lua chunk with this table and ran unluac 2025_12_23 on each one (M1 plan Task 7; DR). Every chunk normalised and decompiled: no chunk was refused, unluac failed on none, and none timed out. **Observed**.

| Directory | Areas | Chunks | Decompiled |
|---|---|---|---|
| `data/scripts` | 16 | 18,855 | 18,855 |
| `data/interface` | 265 | 1,006 | 1,006 |
| `data/core` | 154 | 515 | 515 |
| `data/locale` | 18 | 67 | 67 |
| total | 453 | 20,443 | 20,443 |

An area is a path's directories, at most four deep, such as `data/scripts/combat/skill`; the per-area counts are in DR. Decompiled means unluac exited cleanly; the output is register-level for stripped chunks (see the trial above), and the disassembly stays the ground truth.

## 5. Entries the exe can't supply

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 5.1 | None. Every registered file decodes, and every Lua chunk parses. | **Observed** | RW |
| 5.2 | The planning finding that 79 large entries are cut at 16 KiB was wrong. The planning probe scanned `.data` for 24-byte records with a u64 size. A multi-piece file's first entry has its continuation flag in that u64's high half, so the scan rejected every multi-piece file. The magic scan still found their first pieces, and on its own a first piece looks like a blob cut short. Walked with the mount table, the 87 multi-piece files read whole. | **Observed** | RV, RW |
| 5.3 | The 18,587 `F6 04 ESC Lua` hits ([`spike-0.md`](spike-0.md) 6.12) are a subset of the chunks' starts. 18,504 are the data starts of single-piece files. The other 83 are the first pieces of the 83 multi-piece Lua files. The remaining 1,856 chunks start with a different LZ4 token. | **Observed** | RS, RV |
| 5.4 | The engine registers every piece, so it reads whole files too (1.10). | **Inferred** | G2, G3 |

## Evidence index

Paths are relative to the repo root. `re/` is git-ignored, so these files exist only on the owner's machines. Each probe takes the pinned exe (and, for IX, `data/_index`) and an output directory, for example `python3 -I re/m1/probes/registry_walk.py ~/fenji-client/bin64/gujianol.exe re/m1/evidence`.

| Key | Path or address | What it holds | Findings |
|---|---|---|---|
| RV | `re/m1/evidence/registry.txt`, `stray-magic.tsv`, `undecodable.tsv`, `duplicate-paths.tsv`, `registry-runs.tsv` (probe `registry_verify.py`) | The planning method: a flat scan of `.data` for file entries. Kept to show what it missed. | 5.2, 5.3 |
| RW | `re/m1/evidence/registry-walk.txt`, `multi-piece.tsv`, `registry-walk-failures.tsv` (empty) (probe `registry_walk.py`) | The mount-table walk: mounts, files, pieces, decode and parse results | 1.1–1.4, 2.1, 2.2, 3.1–3.3, 5.1, 5.2 |
| RS | `re/m1/evidence/registry-stats.txt` (probe `registry_stats.py`) | Counts by extension and directory, the login addon and `load.config` names, header field values, the strip check, the stray magic hits | 1.5–1.8, 2.1, 2.3, 2.7, 3.4, 5.3 |
| XO | `re/m1/evidence/extra-opcodes.txt` (probe `extra_opcodes.py`) | Opcode counts, and how the extra opcodes sit: neighbours, operands, jump targets | 3.9, 4.1–4.6 |
| IX | `re/m1/evidence/index-and-opnames.txt` (probe `index_opnames.py`) | The `_index` header, the opcode-name table, every function's last opcode, timestamps as dates | 2.4, 2.6, 3.5, 3.6, 3.8 |
| NT | `re/m1/evidence/normalise-trial/` (probe `normalise_trial.py`); the planning run in `re/m1-planning/sample*.naive.lua` | The normalisation trial and its unluac output | §4 trial |
| PL | `re/m1/probes/probe_*.py` | The planning probes, kept as evidence | 5.2 |
| DR | `re/m1/decompiled-all/decompile-summary.tsv` (`luatool decompile --out re/m1/decompiled-all`) | Per-area decompile counts over every Lua chunk | §4 Decompile results |
| G1 | FUN_140690410; mount table 0x143197720 | Mounts the embedded directories | 1.1, 1.9 |
| G2 | FUN_140690380 | Walks a directory's file entries and joins multi-piece files (piece size 0x3FFC) | 1.2, 1.3, 1.10, 5.4 |
| G3 | FUN_1400B8DE0, FUN_1400B8C10 | Builds an in-memory directory and adds a file: header check, piece array, size | 1.9, 1.10, 5.4 |
| G4 | FUN_1400B4170 | Mounts a node at a path and replaces a same-named node | 1.9, 1.11, 1.13 |
| G5 | FUN_14068FB60 (`--fs=` at 0x142DA5B20), FUN_1400B6580 | File-system set-up: the install root, `--fs=` parsing, archive index loading (FUN_1400BE960), then the embedded mount | 1.12–1.14 |
| G6 | FUN_140136420 (`luaV_execute`): dispatch 0x1401364DB, jump table 0x14013878C, cases 0x140136502 (0), 0x140136511 (1), 0x140136520 (2), 0x14013866B (38) | The VM loop | 3.7, 3.8 |
| G7 | FUN_140136090 (case 49), FUN_140136180 (case 50), FUN_1401362E0 (case 53); cases 0x1401385B1 (48, 52), 0x140138627 (51), 0x1401364AC (46, 47) | The extra opcodes | §4, 4.4, 4.7 |
| G8 | FUN_140135F70, FUN_140135E50 (the lookup), FUN_140136000 (the cache) | The TRY lookups and the function cache | §4 |
| G9 | Opcode-name table 0x142EF9DD0; `'tryself': self is nil!` at 0x142F02E08 | Opcode names | 3.6, §4 |
