# The retranslation: how the client loads its locale cache

The retranslation ([`../superpowers/specs/2026-10-18-retranslation-design.md`](../superpowers/specs/2026-10-18-retranslation-design.md) §6.4–6.5) needs two ways to put new English in front of the client:
- an early check's loader, for development only (RT16);
- the patcher's content hook (#336).

Both start from one question: how the client finds and reads `data/caches/locale_en.new_cache`. This document answers it from the pinned build, statically, and rules on the early check's loader (§4).

**Method.**
- **Build:** the pinned build 10015727, `bin64/gujianol.exe` SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678` ([`pinned-build.md`](pinned-build.md)).
- **Ghidra:** the shared project, read-only (no renames, comments, scripts or debugger), to explore. Each function cited was then read through REA ([`README.md`](README.md#the-norm)). The Evidence IDs are in the [Evidence index](#rea-evidence-ids).
- **Scripts:** the client's own, decompiled locally in M1. Script evidence cites the script's engine path.
- Written by a session that has not read any source rejected in [`sources.md`](sources.md). It holds no game text.

| Tag | Meaning |
|---|---|
| **Observed** | Read from the pinned build's files by our tools or a probe. Anyone with the pinned build can reproduce it. |
| **Inferred** | Our reading of observed facts, including our reading of code in Ghidra or the scripts. Not confirmed at run time. |
| **Unknown** | Open. |

## 1. The scripts' side

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 1.1 | `locale_config` sets the client's language, `__client_locale_language`, from the engine's `locale_get_client_language()`, or to its own default, `de`, when that gives nothing. It applies a translation (`__client_locale_apply_language`) for any language but `zh_CN`. | **Inferred** | `data/core/share/locale_config.luac` |
| 1.2 | `locale` loads `data/caches/locale_<language>.new_cache` through the scripts' file system, `__script_system_root_vfs`: `stat`, then `load`, then `ks.pickle_unpack`. The context `""` becomes the main table, and the cache's other contexts stay by name ([`m2-tables.md`](m2-tables.md) 2.6). | **Inferred** | `data/core/share/locale.luac` |
| 1.3 | If the cache doesn't `stat`, `locale` reads `data/tables/locale/<language>.tab` instead, through `load_tab`, by its Origin, Translation and LocaleEnv columns. If neither file exists, no translation applies. `force_set_client_language` sets the language and loads again. | **Inferred** | `data/core/share/locale.luac` |

## 2. The language Fenji's start gives

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | `locale_get_client_language` (FUN_1401E84F0, registered with the platform functions by FUN_1401E8710) returns the engine's language string at 0x143113AC8. A static constructor sets it to `de` (FUN_14005D5D0). | **Inferred** | FUN_1401E8710, FUN_1401E84F0, FUN_14005D5D0 |
| 2.2 | At start-up, FUN_1401E7D10 maps the language of the game's platform record (FUN_1401A37B0, its string at +0x28) to one of four: `en-US`, `en-GB` and `english` give `en`; `de-DE` and `german`, `de`; `fr-FR` and `french`, `fr`; `ru-RU` and `russian`, `ru_RU`. Anything else gives `en`, the empty string among them. | **Inferred** | FUN_1401E7D10 |
| 2.3 | The platform record always exists, and its language starts empty (FUN_1401A37B0); only the Gameforge SDK fills it ([`../protocol/login.md`](../protocol/login.md) G10). On the launcher's path, `gj_uinfo` skips the SDK (`login.md` 2.9), so the language is `en` and the client loads `locale_en`. That matches what M1 saw: English on that path, German with the mocked SDK's `de-DE` ([`m1-summary.md`](m1-summary.md) 8.19). | **Inferred**, matching **Observed** runs | FUN_1401A37B0, FUN_1401E7D10 |

## 3. The engine's file system

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 3.1 | `__script_system_root_vfs` is the engine's root file system: FUN_14068FB20 returns it (0x143A4A340), and FUN_140250140 sets it as the scripts' global. Its methods, in the table at 0x142EF8510, are `__gc`, `__rawptr`, `destroy`, `stat`, `checksum`, `copy`, `remove`, `open`, `list`, `iter`, `dir`, `save`, `load`, `load_utf8` and `load_tab`. | **Observed** (the table, read in the shared project); its use **Inferred** | FUN_140250140 |
| 3.2 | `load` (FUN_1400EB3C0) opens the path through FUN_1400B3390 and reads the file whole. `stat` (FUN_1400E9CC0) asks the same tree, through FUN_1400B31A0. FUN_1400B3390 walks a tree of nodes from the root: the deepest node on the path that has a file system attached opens the rest of the path. A failed open there doesn't fall back to a shallower one. | **Inferred** | FUN_1400EB3C0, FUN_1400E9CC0, FUN_1400B3390 |
| 3.3 | FUN_14068FB60 builds the root from the argument `--fs=<folder>:<data index>:<asset index>` ([`../protocol/login.md`](../protocol/login.md) §1), in the copy's root: the exe's folder's parent (FUN_1400B6580). Fenji starts the client with `--fs=data:_index:_index`. | **Inferred** | FUN_14068FB60 |
| 3.4 | With those arguments, the root is the archive file system. Its one store holds the parts `data/data000` to `data255` that exist, then every file in `<copy>/patch/`, as parts too (FUN_1400C0A90, FUN_1400C0CF0, FUN_1400BE8A0). FUN_1400BE960 parses `data/_index` into its tree. A folder's file system for `<copy>/tools` hangs under the root as the node `tools` (FUN_1400B7D20; FUN_1400B4170 attaches a file system under the name it was given). `asset` is a view of the same tree, since the two indexes are one (FUN_14068FB60). | **Inferred** | FUN_1400C0A90, FUN_1400C0CF0, FUN_1400BE8A0, FUN_1400BE960, FUN_14068FB60, FUN_1400B7D20, FUN_1400B4170 |
| 3.5 | An `_index` line is `<40 hex digits>\t<path>` ([`m1-archives.md`](m1-archives.md) 3.2). FUN_1400BE4A0 attaches the path to the first store that holds the key, and a read opens it there (FUN_1400BDE90). A key no store holds makes no node, so its path doesn't exist. A line that starts with `-` removes a path. | **Inferred** | FUN_1400BE4A0, FUN_1400BDE90 |
| 3.6 | A later line for a path re-points it: FUN_1400BE3A0 returns the existing file node, and FUN_1400BE4A0 writes the new store and key over it. | **Inferred** | FUN_1400BE3A0, FUN_1400BE4A0 |
| 3.7 | A record loads only if its key is new ([`m1-archives.md`](m1-archives.md) 1.5), and the patch folder's files come after the parts. So a patch part adds files, and can't shadow one. | **Inferred** | FUN_1400C0A90; `m1-archives.md` G2 |
| 3.8 | So the engine never reads a loose file under `data/`: a path there resolves only through `_index`'s nodes to a store's record. From disk it reads only the parts, the patch folder's files, the index files `--fs` names, and the `tools` mount (and `asset`, when the third field is empty). | **Inferred** | 3.2–3.7 |
| 3.9 | An index file is a blob opened from disk (FUN_1400BE960, then FUN_1400BC3A0). Its 32-byte header ([`m1-script-format.md`](m1-script-format.md) §2) must pass FUN_1400BC230: a CRC-16 of its first 30 bytes in the last two (with the CRC-16/ARC table at 0x142EF7EC0 that records use, `m1-archives.md` G2), a stored size of 32 or more, and a block size from 128 bytes to 2 MiB. Codec 0 is stored: the data follows the header as it is. Codecs 1 and 2 share one decoder, 3 has another, and 4 and up are Oodle's ([`m1-archives.md`](m1-archives.md) G5). The copy's own headers carry that check: `_index`'s, `0xB355` ([`m1-archives.md`](m1-archives.md) 3.1), and the first five records of `data000`, each the CRC-16/ARC of its first 30 bytes. | **Inferred**; the check **Observed** on the copy's headers | FUN_1400BC3A0, FUN_1400BC230; probe `re/retranslation/probes/blob_check.py` (its output `blob_check.txt` beside it) |
| 3.10 | A third `--fs` field that differs from the second loads a second index into the same tree after `_index`, and its lines win (3.6). The four readers of that field's name use it only as a flag, "the assets are packed" (FUN_140278FC0, FUN_1402790F0, FUN_140689360, FUN_140651D40). But the engine also hands whether the two fields are the same (FUN_140690350) to its start-up's renderer set-up: FUN_140191750 calls FUN_140665670 with it, for a reason not traced. | **Inferred**; the flag's use **Unknown** (U1) | FUN_14068FB60, FUN_140278FC0, FUN_1402790F0, FUN_140689360, FUN_140651D40, FUN_140690350, FUN_140191750 |
| 3.11 | A `*` before the first field backs the store with a folder of loose files instead of the parts (FUN_1400D1CC0, from FUN_1400BE8A0): the whole game unpacked, not a way to swap one file. | **Inferred** | FUN_1400D1CC0, FUN_1400BE8A0 |

## 4. The early check's loader

The ruling (the plan's Task 9, step 2), among its three options:
- **(b), refined: a sandbox-private overlay of the copy, started with Fenji's own arguments** (`--fs=data:_index:_index`), and holding two files of its own:
  - `patch/<name>`: a part ([`m1-archives.md`](m1-archives.md) §1: the magic, one `[IX]` block, one in-use record) that holds the new cache as a stored blob, keyed by its SHA-1 (3.4, 3.7);
  - `data/_index`: the copy's own, decoded, with one line added at the end that re-points `data/caches/locale_en.new_cache` to the new key (3.6), written back as a stored blob (3.9).
- **Not (a), a loose file:** the engine reads none under `data/` (3.8).
- **Not a second index in `--fs`'s third field:** it would re-point the path as well (3.10), but it changes a flag the engine hands on, whose use isn't traced (U1). (b) keeps the run a player's in everything but the two files.
- **Not (c), a debugger's swap of the read buffer:** it puts a debugger in the loop, which (b) doesn't need.

Everything stays inside the sandbox (rule 4), and the copy itself is never written: the overlay holds the two files. The patcher's content hook (#336) faces the same choices for a player's copy, and its spec decides from this.

## 5. Unknowns

| # | Question | What would answer it |
|---|---|---|
| U1 | What FUN_140665670 does with the "one index" flag (3.10) | Its callees, statically. It matters only to a second index. |
| U2 | Whether the engine checks a record's content against its key | A sandboxed run with a stored record under a wrong key. The loader writes the right SHA-1 anyway. |
| U3 | That §3 holds at run time | The early check itself (the plan's Task 11). |

## REA Evidence IDs

Read through REA 6.3.0 with Ghidra 12.1.4 on the pinned exe, in one session on 2026-10-18, after the findings had been written from the shared Ghidra project. The session's setup check, `procedure_pseudo_code {procedure: "0x14029C330"}`, gave `ev_2487ebf9c2001ca037d52f138681f91175ece7bc1de0ba6422232f62d2ca9b77`, as the recipe says ([`README.md`](README.md#recipe)). Each reading shows the function in the role the finding gives it. An ID covers its call exactly as sent, so anyone can check it by repeating the call ([`README.md`](README.md#how-to-cite)).

| Function | Cited in | Call | Evidence ID |
|---|---|---|---|
| FUN_1401E8710 | 2.1 | `procedure_pseudo_code {procedure: "0x1401e8710"}` | `ev_b8833eb2aa9fb91ea60cecff0721a55e0383259d1639a3e5b336421730588994` |
| FUN_1401E84F0 | 2.1 | `procedure_pseudo_code {procedure: "0x1401e84f0"}` | `ev_4e5f4ef3a94c78c9eb77185d7f6baec87193d039215d0c3e464d59fe9d4ce7d8` |
| FUN_14005D5D0 | 2.1 | `procedure_pseudo_code {procedure: "0x14005d5d0"}` | `ev_e0adceb5e36fda38ea07383d9caf1fd992ac85c86b01c4e0cf5e0aa5f0e0ce72` |
| FUN_1401E7D10 | 2.2, 2.3 | `procedure_pseudo_code {procedure: "0x1401e7d10"}` | `ev_8306bd868f544054b369e8eb2e5ab5d43c02b4a52da93291596ff704d0aa5097` |
| FUN_1401A37B0 | 2.2, 2.3 | `procedure_pseudo_code {procedure: "0x1401a37b0"}` | `ev_70cb7a254e6ce5e723ab099070b95cfd347321a7a7b7c51519dfaa0d31437bf9` |
| FUN_14068FB20 | 3.1 | `procedure_pseudo_code {procedure: "0x14068fb20"}` | `ev_9b5a23cee67ab0ee3b3304c24f48d4ce605872736da28dee2bb0f6078e0bce6b` |
| FUN_140250140 | 3.1 | `procedure_pseudo_code {procedure: "0x140250140"}` | `ev_6a0f5a0029ba0843fddbc43ae6c8d1dec5023d93e358bb75c20df9c0f5f1b7af` |
| FUN_1400EB3C0 | 3.2 | `procedure_pseudo_code {procedure: "0x1400eb3c0"}` | `ev_edba797dc4a191d98c33199f817b342fb9828a5cf0ed70eb287842bbe0038797` |
| FUN_1400E9CC0 | 3.2 | `procedure_pseudo_code {procedure: "0x1400e9cc0"}` | `ev_64128becc7baacc29ec57dc88a63c41214d41c0390cc356fadd9cc9fce1a00cc` |
| FUN_1400B31A0 | 3.2 | `procedure_pseudo_code {procedure: "0x1400b31a0"}` | `ev_17e003a8f4937824fb7c45ad9e97e00555b083e8a10102d591e251e844192e86` |
| FUN_1400B3390 | 3.2 | `procedure_pseudo_code {procedure: "0x1400b3390"}` | `ev_5be30e9e89449b806949cf2312f4ceeb2162b665b2669ec84b2814d27b7b8996` |
| FUN_14068FB60 | 3.3, 3.4, 3.10 | `procedure_pseudo_code {procedure: "0x14068fb60"}` | `ev_cdbf9f0b5f8e4a89dfd0f0f24b5922c2ab03a454140354e4121ee292b9ef2af1` |
| FUN_1400B6580 | 3.3 | `procedure_pseudo_code {procedure: "0x1400b6580"}` | `ev_e9159370cb6508412fb2516ac5082c00a068a65a3090b50bfdc075c748343fdf` |
| FUN_1400C0A90 | 3.4, 3.7 | `procedure_pseudo_code {procedure: "0x1400c0a90"}` | `ev_1249658f76da6408890e5b33d21153f0ac28224c98419e8db0e1a3f360129353` |
| FUN_1400C0CF0 | 3.4 | `procedure_pseudo_code {procedure: "0x1400c0cf0"}` | `ev_1cab61bde02a752054bcc6bfd8c867faeb10e828ed45ec03624c105caa9223fe` |
| FUN_1400BE8A0 | 3.4, 3.11 | `procedure_pseudo_code {procedure: "0x1400be8a0"}` | `ev_f1ead1058e82742919193883738767c6f28fa314a7bfc14c8296a0214e8c53db` |
| FUN_1400BE960 | 3.4, 3.9 | `procedure_pseudo_code {procedure: "0x1400be960"}` | `ev_00fe3f3bd249e3278744fed1d8eee504ae92926932524708932fda497684589d` |
| FUN_1400B7D20 | 3.4 | `procedure_pseudo_code {procedure: "0x1400b7d20"}` | `ev_848f63cfdc6dd3415ed5a9bd66b2f8874e2d456730d73dc0970b7d0f1944a014` |
| FUN_1400B4170 | 3.4 | `procedure_pseudo_code {procedure: "0x1400b4170"}` | `ev_65f7b3a51e0b0827b641b11be75ea1d3e4c31a2407e4eb0886985591e7e2f7ca` |
| FUN_1400BE4A0 | 3.5, 3.6 | `procedure_pseudo_code {procedure: "0x1400be4a0"}` | `ev_b2b3685a8d5b684b5f7cf76a29eae6f4d4896b0e41ed33a28e25ca99ef7e252c` |
| FUN_1400BDE90 | 3.5 | `procedure_pseudo_code {procedure: "0x1400bde90"}` | `ev_bc810f2a5c9c8f73989f5410807157279ca37e762d70d2ffb3e576129d58e0b5` |
| FUN_1400BE3A0 | 3.6 | `procedure_pseudo_code {procedure: "0x1400be3a0"}` | `ev_b516c6581e49fbfe4206dd4e5741b6c733bd9f1603e78a9ade766d2ec8970387` |
| FUN_1400BC3A0 | 3.9 | `procedure_pseudo_code {procedure: "0x1400bc3a0"}` | `ev_383cfc027b30d58d8fb649f8a8989533bc057eff8054bf82b02fe453e4ba6748` |
| FUN_1400BC230 | 3.9 | `procedure_pseudo_code {procedure: "0x1400bc230"}` | `ev_96b6a6d58685663de96cee7df5e1a52046181071d72ea66d77f8573182eda0c4` |
| FUN_140278FC0 | 3.10 | `procedure_pseudo_code {procedure: "0x140278fc0"}` | `ev_4d014f76c5436043708da2452220284afb2d4d44878c72f1a59af9ea918074e8` |
| FUN_1402790F0 | 3.10 | `procedure_pseudo_code {procedure: "0x1402790f0"}` | `ev_fad241a753ffdb0ebefa162d10fc548299d449d46a3fd6cd57997065682db388` |
| FUN_140689360 | 3.10 | `procedure_pseudo_code {procedure: "0x140689360"}` | `ev_ca86931bb0f6228db22dbfb5302e2c6e4c9abdb3067cc47e8e3ac000c43d6821` |
| FUN_140651D40 | 3.10 | `procedure_pseudo_code {procedure: "0x140651d40"}` | `ev_dd52252ceb06872457a34ca547c67eaa73153b3016bec82bbe1282b4b74f1cd2` |
| FUN_140690350 | 3.10 | `procedure_pseudo_code {procedure: "0x140690350"}` | `ev_998bdddb9b6fa7e739ba49b8aaf0c9eba975f0be0e73630142d6f4d0bf708528` |
| FUN_140191750 | 3.10 | `procedure_pseudo_code {procedure: "0x140191750"}` | `ev_3da4a67b13d0f8defbd0064f0bf5c3381aea7e383f49c311541344d687462827` |
| FUN_1400D1CC0 | 3.11 | `procedure_pseudo_code {procedure: "0x1400d1cc0"}` | `ev_742778a590464fb047b9d8cb925c42006fb81a444485bdb95481a31cec6d2ae2` |

The session's bundle is `re/retranslation/rea/bundle-1.json` (32 records), on the owner's machine.
