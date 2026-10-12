# M1: the data archives without decompression

`archive` now reads the client's 100 data parts completely, without decompressing anything. Each part is a chain of checksummed index blocks, and each 32-byte record gives a file's SHA-1, its offset and its size. Of the 224,032 files, 93.6% are Oodle-compressed. The other 14,437 (23.8 GB) are stored uncompressed and readable today: videos, sound banks and a large set we can't identify yet. Most files are named only in `_index`, which is one Oodle blob, so most paths wait on decision gate #39. Two stored containers already name 16,162 paths in plain text.

**Method.**
- **Build:** the pinned build 10015727, `bin64/gujianol.exe` SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678` ([`pinned-build.md`](pinned-build.md)).
- **Ghidra:** queried read-only through the shared project (no renames, comments, scripts or debugger) for the engine's own part reader.
- **`archive`** (`tools/archive`) reads each part's index blocks and the 32-byte blob headers its records point at, and extracts stored files.
- **Python probes** (standard library) measured what the tool doesn't print.
- **Nothing was decompressed:** Oodle (codec 4) waits on gate #39.

| Tag | Meaning |
|---|---|
| **Observed** | Read from the pinned build's files by `archive` or a probe. Anyone with the pinned build can reproduce it. |
| **Inferred** | Our reading of observed facts, including our reading of code in Ghidra. Not confirmed at run time. |
| **Unknown** | Open. |

Evidence keys such as `AI` or `G2` resolve in the [Evidence index](#evidence-index). File keys point under `re/m1/` (git-ignored, on the owner's machines), and `G` keys are addresses in the pinned exe.

In short:
- **Layout:** a part is the magic `ZFS\0` followed by a chain of index blocks of 4,096 records each. A record is a key, an offset, a size, a CRC-16 and a status byte (§1). **Observed** and **Inferred**.
- **Key:** a record's key is the SHA-1 of the file's content, and the index maps paths to keys (§2, §3). **Observed** for stored files.
- **Spike-0's puzzle:** it read only the first index block and took deleted records for live ones, which is why its records didn't tile the parts. Read whole, the chains account for all but 0.37% of the bytes (§2). **Observed**.
- **Codecs:** 209,595 files are Oodle, 14,437 stored, and none LZ4 (§2). **Observed**.
- **Stored files:** mostly Bink videos and FMOD Studio banks, plus 14,041 files in a format we don't know yet (§4). **Observed** and **Inferred**.
- **What Oodle would unlock:** the paths of the archives' files and the content of 93.6% of them. The exe's own scripts don't need it, since they are complete (§5). **Inferred**.

## 1. Part layout

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 1.1 | Each of the 100 parts `data000`–`data099` begins with the 4-byte magic `ZFS\0`. The engine checks it when it opens a part, and writes it when it creates one. | **Observed**; the engine's use **Inferred** | AI, G1 |
| 1.2 | The first index block starts at offset 4 and is 0x20008 bytes long, so the data starts at 0x2000C. It holds `[IX]`, then a u32 offset of the next index block (0 ends the chain), then 4,096 records of 32 bytes. The "4-byte field" in [`spike-0.md`](spike-0.md) 6.2 is that next-block offset. | **Observed** | AI, G2 |
| 1.3 | The index blocks form a chain. 19 parts have more than one block, and the longest chain is 6 blocks (`data072`, 22,272 live records). There are 131 blocks in all; the later blocks sit among the data. | **Observed** | AI |
| 1.4 | A record is: a 20-byte key, a u32 data offset, a u32 size, a u16 CRC and a status byte. The CRC is CRC-16/ARC (reflected polynomial 0xA001, initial value 0) over the first 28 bytes. The byte before the status is never read and is 0 in every record. | **Observed** (fields; the CRC checks on all 338,842 non-free records); meanings **Inferred** from the reader | AP, G2 |
| 1.5 | Status 0 means free, and the engine stops reading that block there. 1 means in use. Any other value means deleted; all 114,810 deleted records hold 0xff. The engine loads an in-use record only if its CRC is good and its key is new. When it may write, it marks a bad or duplicate record 0xff and frees a deleted record's range. | Counts **Observed**; behaviour **Inferred** | AI, G2 |
| 1.6 | The engine keys its in-memory table by the 20-byte key (Bob Jenkins' lookup2 hash). Each loaded file keeps one part number, offset and size, so no file spans parts. | **Inferred** | G2 |
| 1.7 | A file whose path ends in `$$` is a container: a stored file that starts with `[SP]` and holds its own listing and records (4.3). | **Inferred** | G3, G4 |

## 2. Records and blobs

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | Over the whole chains there are 224,032 live records and 114,810 deleted ones. None has a bad CRC. | **Observed** | AI |
| 2.2 | Every live record's size equals the stored size in the 32-byte blob header at its offset, and every one of those headers parses. | **Observed** | AI |
| 2.3 | The live files' codecs: 209,595 Oodle (codec 4, 93.6%) and 14,437 stored (codec 0, 6.4%). There is no LZ4 and no other codec, so the exe's embedded files are the client's only LZ4 blobs. | **Observed** | AI |
| 2.4 | A stored blob has no block list: its content follows the 32-byte header. Its stored size is its raw size plus 32 in all 14,437 cases. The block-size field holds min(raw size, 512 KiB) in 14,222 of them. | **Observed** | AP |
| 2.5 | The key is the SHA-1 of the file's content. That holds for all 1,135 stored files that `archive extract` wrote from parts 24, 93 and 99, and for 40 of 40 in an earlier sample. It is never the SHA-1 of the whole blob. For compressed files it is presumably the SHA-1 of the decompressed content. **M2 update:** confirmed for six Oodle files, which `archive extract --key` decoded through the Oodle helper ([`m2-tables.md`](m2-tables.md) 1.6). | **Observed** (stored; six Oodle files in M2); **Inferred** (the other compressed files) | AX, AP; m2-tables.md TX |
| 2.6 | The blob header's `check` field is not a CRC-16/ARC of the header's first 28 bytes. | **Observed** | AP |
| 2.7 | The 247,313 "non-empty records" in [`spike-0.md`](spike-0.md) 6.2 are exactly the non-free records (live and deleted) in each part's first index block. Read whole, the chains hold 338,842. | **Observed** | AP |
| 2.8 | [`spike-0.md`](spike-0.md) 6.3–6.4 asked why the records tile only 10 parts. Live records and index blocks tile 7 parts exactly. Adding the ranges of deleted records leaves gaps of 392,810,956 bytes (0.37% of the 100 parts). In 77 parts, deleted ranges overlap live ones. | **Observed** | AP |
| 2.9 | So the space a delete frees gets reused, and the remaining gaps are space whose deleted record was itself reused. The record format is understood; 6.4's Unknown is closed. | **Inferred** | AP |

## 3. `_index`

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 3.1 | `_index` is a single blob: raw size 25,533,513; stored size 5,736,039 (the file's size); timestamp 2022-11-25 09:28:58 UTC; block size 512 KiB; codec 4 (Oodle); check 0xB355; 49 blocks. | **Observed** | AN |
| 3.2 | The engine opens `_index` through its blob stream and parses lines of `<40 hex digits>\t<path>`. The hex is a file's key, which it looks up in the parts' records; the path becomes a node in the file system. A line that starts with `-` removes a path, and a path that ends in `$$` is a container. | **Inferred** | G3 |
| 3.3 | So outside the two containers, `_index` is the only place the archives' paths are named, and reading it needs codec 4. | **Inferred** | G3, AI |
| 3.4 | **M2 update: the blocks and their decoder.** Each of `_index`'s 49 blocks starts with its raw length as a little-endian `u32` (0x80000; the last block 0x59C49), then the Oodle stream. The engine's four Oodle codecs, Kraken, Mermaid, Selkie and Leviathan, share one decompressor, FUN_14024E060, which passes `OodleLZ_Decompress` the block after those 4 bytes, with the `u32` as the raw length. `tools/oodle-helper` does the same with the client's own `oo2core_6_win64.dll`, inside the sandbox (M2 plan Task 11). All 49 blocks decode to their raw lengths. The joined 25,533,513 bytes are 272,741 lines of `<40 hex digits>\t<path>` and a final newline. 2 of the paths end in `$$` (containers), no line starts with `-`, and 7 paths aren't valid UTF-8. | **Observed** (the prefixes, the decode, the lines); the shared decompressor **Inferred** | AN, AO; G5 |
| 3.5 | **M2 update: the paths.** `archive paths` (M2 plan Task 12) decodes `_index` through the spool and looks each path's key up among the parts' live records. `_index` leaves 272,734 distinct paths, 2 of them containers, with no bad line and no removal; the 7 lines that aren't UTF-8 aren't mapped (3.4). 272,638 paths, with 224,032 distinct keys, name a live record, and every live record is named at least once: `_index` names every file in the parts, many under more than one path. Of those paths, 31,760 lead to stored files and 240,878 to Oodle files. The other 96 paths, with 82 keys, lead to no record: no live one, no deleted one, and no file in the two containers. They are the client's loose files, 42 under `bin64/` and 54 under `tools/`: each is on disk with the SHA-1 its key names. Of the copy's 109 loose files, `_index` names those 96 at their path and not the other 13 (2 `.exe`, 7 `.dll`, 4 `.ini`). | **Observed** | AQ |

## 4. Stored files

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.1 | The 14,437 stored files (23.8 GB) sort by their first bytes into the five kinds in the table below. Nothing starts like a Lua chunk, XML, text or a common image or archive format. | **Observed** (first bytes); the formats **Inferred** from their magics | AK |
| 4.2 | So the stored files are media the packer didn't compress, plus a large set we haven't identified. | **Inferred** | AK |
| 4.3 | Two stored files are containers (`data093`: 13,179 files and a 1,089,311-byte listing; `data099`: 2,983 files and 248,064 bytes). Each has the layout in the second table below. Every header CRC and every record checks, and every record's key appears in its listing. | **Observed** (layout and checks); meanings **Inferred** | AC, G4 |
| 4.4 | All 16,162 files inside the containers are Oodle-compressed. Most are under `asset/avatar` (11,948 of `data093`'s) and `asset/materials` (2,972 of `data099`'s). These listings name 16,162 paths today, but the files' contents wait on gate #39 too. | **Observed** | AC |

The five kinds of stored file (4.1):

| Kind (first bytes) | Files | Size |
|---|---|---|
| Bink 2 video (`KB2j`) | 278 | 12.5 GB |
| FMOD Studio bank (RIFF form `FEV `) | 113 | 6.0 GB |
| Bink 1 video (`BIK`) | 3 | 262 MB |
| Container (`[SP]`) | 2 | 16.7 MB |
| Unidentified: 13,430 different first four bytes, matching no format we checked | 14,041 | 5.1 GB |

A container's layout (4.3):

| Part | Contents |
|---|---|
| Header (18 bytes) | `[SP]`, the file count, the listing's length, the records' offset (18 + the listing's length), and a CRC-16/ARC of those 12 bytes |
| Listing | Plain text: one `<sha1 hex>\t<path>` line per file |
| Records | One 32-byte record per file in the part layout, with offsets relative to the container's content |

## 5. What Oodle would unlock

**Inferred:**
- **Path names.** `_index` names the paths of the archives' 224,032 live files. With it, `archive` could list every file by name, and extract the stored ones by path.
- **Content.** 93.6% of the files, including the game's assets and whatever tables the server must share with the client.
- **Not missing scripts.** The exe's embedded scripts are complete ([`m1-script-format.md`](m1-script-format.md) §5.2), so they don't need Oodle. Any script copies in the archives would be shadowed, since the embedded copies win (same document, 1.13). Whether any exist is Unknown until `_index` is readable.

**Unknown:**
- **The unidentified stored files.** What the 14,041 are.
- **Encryption.** Whether any compressed file is also encrypted ([`spike-0.md`](spike-0.md) 6.7). The 1,135 stored files checked are plain, since their SHA-1s match their keys. The rest weren't checked; that includes the 14,041 unidentified ones, whose first bytes vary widely.
- **Unexplained fields.** The meaning of the blob header's `check` field, and of the always-zero byte at record offset 0x1E.

## Evidence index

Paths are relative to the repo root. `re/` is git-ignored, so these files exist only on the owner's machines. The tool's commands take the client copy from `$FENJI_CLIENT_PATH`, for example `FENJI_CLIENT_PATH=~/fenji-client target/release/archive info > re/m1/archive-info.tsv`. The probes take the client's `data/` directory, for example `python3 -I re/m1/probes/zfs_probe2.py ~/fenji-client/data re/m1/archive-probe.tsv`.

| Key | Path or address | What it holds | Findings |
|---|---|---|---|
| AI | `re/m1/archive-info.tsv` (`archive info`) | Per part: index blocks, records by state, live records by codec, containers, mismatches | 1.1–1.3, 1.5, 2.1–2.3, 3.3 |
| AN | `re/m1/archive-index.txt` (`archive index`) | `_index`'s blob header | 3.1 |
| AX | `re/m1/archive-stored/data024/`, `data093/`, `data099/` (`archive extract --stored-only`) | Stored files named by key; the key checks it printed | 2.5 |
| AP | `re/m1/archive-probe.tsv`, `re/m1/archive-notes.md` (probes `re/m1/probes/zfs_probe.py`, `zfs_probe2.py`; `archive list` for 2.7) | Record fields and CRCs, statuses, deleted ranges, tiling, stored blob shape, the key and `check` tests, the spike-0 reconciliation | 1.4, 2.4–2.9 |
| AK | `re/m1/archive-stored-kinds.txt`, `re/m1/archive-stored-sizes.txt` (probe `re/m1/probes/stored_kinds.py`) | Stored files by first bytes and size | 4.1, 4.2 |
| AC | `re/m1/archive-notes.md` (probes `re/m1/probes/sp_probe.py`, `sp_probe2.py`) | The two containers | 4.3, 4.4 |
| AO | `re/m2/oodle-helper-run2.log` (`scripts/spike/run-oodle-helper.sh`), `re/m2/index-check.txt` (from `re/m2/probes/check_index.rs`, run as a `fenji-data` example), `re/oodle-spool/` | The first decode with the client's DLL, and the shape of the joined `_index` | 3.4 |
| AQ | `re/m2/archive-paths.txt` (the summary of `archive paths --out re/m2/archive`), `re/m2/archive/paths.tsv`; `re/m2/archive-paths-stats.txt`, `re/m2/missing-keys.txt`, `re/m2/deleted-keys.txt` and `re/m2/container-keys.txt` (probe `re/m2/probes/paths_stats.sh`); `re/m2/unfound-on-disk.txt` (probe `re/m2/probes/unfound_on_disk.py`); `re/m2/loose-files.txt` (probe `re/m2/probes/loose_files.py`) | The paths mapped to records, the 96 that are loose files, and the loose files `_index` doesn't name | 3.5 |
| G1 | FUN_1400BF780, magic at 0x142EFC778; FUN_1400C0A90 (`%s/data%03u` at 0x142EFC7E8) | Opens part N and checks or writes `ZFS\0`; builds the part paths | 1.1 |
| G2 | FUN_1400C0FE0, FUN_1400BFF10; `[IX]` at 0x142EFC77C; CRC table 0x142EF7EC0; messages at 0x142EFC788, 0x142EFC7A8, 0x142EFC7C8 | Loads every part's index chain: record fields, CRC, status, the key table | 1.2, 1.4–1.6 |
| G3 | FUN_1400BE960, FUN_1400BE4A0 | Loads `_index` through the blob stream and parses its lines | 1.7, 3.2, 3.3 |
| G4 | FUN_1400BEB00; `[SP]` at 0x142EFC770; FUN_1400B84E0 (the header CRC) | Reads a container | 1.7, 4.3 |
| G5 | 0x14068EFC0 (not a function in the shared project yet): a `u16` selector, 0 to 3, giving the compressor of Kraken, Mermaid, Selkie or Leviathan (FUN_14024E0C0, FUN_14068F5A0, FUN_14024E200, FUN_14024E160; names at 0x1411927B0–0x1411927C8) and, for all four, the decompressor FUN_14024E060. REA (after the evidence norm, [`README.md`](README.md#how-to-cite)): FUN_14024E060, `procedure_pseudo_code {procedure: "0x14024e060"}`, `ev_2107c333327719b3e0b3e2da90b07a2ab6a172721f95a3dce76a68b175b7356f`, reads the block's `u32` raw length and passes the rest to `OodleLZ_Decompress`; at 0x14068EFC0 REA has no function either (`README.md`, Pitfalls), so it is cited by address | The engine's Oodle codecs and their shared decompressor | 3.4 |
