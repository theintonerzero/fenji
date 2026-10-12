# M2: the client's tables

The client doesn't ship its design tables (items, skills, NPCs, quests, maps and the rest) as `.tab` files. It ships them compiled into one cache, `data/caches/all_tables.new_cache`. The cache is a single value in the client's own pickle format ([`../protocol/payloads.md`](../protocol/payloads.md)), with two features the network messages don't use: tables referenced from several places, and "magic" objects rebuilt by constructors. With the Oodle helper (M2 plan Task 11) and `_index` (Task 12), the whole cache decodes, and it holds 339 sheets, items, skills, NPCs, the experience curve and maps among them. The few `.tab` files that do ship are UTF-8 text with three header lines.

**Method.**
- **Build:** the pinned build 10015727, `bin64/gujianol.exe` SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678` ([`pinned-build.md`](pinned-build.md)).
- **Ghidra:** the shared project, read-only (no renames, comments, scripts or debugger), for the unpacker.
- **Scripts:** the client's own, decompiled locally in M1. Script evidence cites the script's engine path.
- **Decoding:** `archive paths` found the files; the Oodle helper decoded their blocks inside the sandbox; local Python probes (standard library) read the decoded caches and printed names and counts only.
- Written by a session that has not read any source rejected in [`sources.md`](sources.md).

| Tag | Meaning |
|---|---|
| **Observed** | Read from the pinned build's files by our tools or a probe. Anyone with the pinned build can reproduce it. |
| **Inferred** | Our reading of observed facts, including our reading of code in Ghidra or the scripts. Not confirmed at run time. |
| **Unknown** | Open. |

Evidence keys resolve in the [Evidence index](#evidence-index). File keys point under `re/m2/` (git-ignored, on the owner's machines).

## 1. Where the tables are

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 1.1 | `_index` names `.tab` files under `data/tables/present`, `cutscene`, `locale`, `combat` and `player`, one at `data/tables/errors.tab`, and two per map directory under `asset/maps`. None of them holds items, skills, NPCs, quests or maps, although the scripts name such files (`data/tables/quest/quest_main.tab`, `data/tables/maps/map.tab`, and `data/tables/item/shop/…` in the exe's strings). Neither `_index` nor the exe's embedded file system holds those paths. | **Observed** | TP, TE |
| 1.2 | `_index` also names `data/caches/*.new_cache`: `all_tables` (13,025,329 bytes stored, 605 Oodle blocks, 316,758,404 bytes raw), `combat_tables`, `prop_tables`, `protocol_ver`, and `locale_<language>` for `de`, `en`, `fr`, `kr`, `ru_RU`, `vi` and `zh_MYSG`. | **Observed** | TP, TD |
| 1.3 | `content_system` loads `data/caches/all_tables.new_cache` through the scripts' virtual file system, unpacks it with `ks.pickle_unpack`, and keeps the result as the global `all_sheets`. If `data/caches/table_diff.patch` exists, it applies that file's entries (each a `key_path`, a `key_list` and a `value`) on top. | **Inferred** | `data/core/client/content_system.luac` |
| 1.4 | The other caches have their own loaders: the locale script reads the locale caches, the actor class registry reads `prop_tables` and `combat_tables`, and the enums script reads `protocol_ver`. | **Inferred** | `data/core/share/locale.luac`, `data/core/xobject/client_actor_class_register.luac`, `data/core/share/enums.luac` |
| 1.5 | So the design tables reach the client as one compiled cache, and the `.tab` files that ship are the few the client reads directly. That the cache is compiled from `.tab` sources is our reading: `combat_level.tab`'s 181 rows match the cache's 181-entry `combat_level` sheet (3.1). | **Inferred** | TD, TT |

## 2. The cache format

A cache is the pickle of [`payloads.md`](../protocol/payloads.md) P.1–P.2, plus the forms below.

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | **Compression.** The small caches (`combat_tables`, `prop_tables`, `protocol_ver`) start with `FF`: LZ4-compressed, as P.1 and P.5 describe. `all_tables` and the locale caches are too large for the packer's 1 MiB bound and are stored uncompressed (their Oodle blobs compress them in the archive). | **Observed** | TD; FUN_1400D53D0 |
| 2.2 | **Table references.** The header's count byte is `0x81`, so one value follows a 24-bit big-endian count N (P.1). The unpacker numbers the first N tables in the order their `0B` tag is read, before their contents, and tags `0E`, `0F` and `10` carry an 8-, 16- or 32-bit index below the number registered so far ("bad tref" otherwise). Such a tag stands for that same table, so a table can appear in several places, an ancestor included. | **Inferred** | FUN_1400D56D0, FUN_1400D4AC0 (cases `0B`, `0E`–`10`) |
| 2.3 | `all_tables` declares 3,749,858 referable tables and registers all of them; `locale_en` declares 29. Both decode with 2.2 and end exactly on their last byte. | **Observed** | TD, TS |
| 2.4 | **Magic objects.** Tag `0D` is followed by a name value and then one argument value. The unpacker calls the constructor registered under that name with `ks.pickle_unpack_magic_setup`, and the result takes the object's place. A `nil` name means the next value stands for itself. | **Inferred** | FUN_1400D4AC0 (case `0D`) |
| 2.5 | `all_tables` holds two kinds: `LString` (354,404) and `Float4` (23,499). | **Observed** | TS |
| 2.6 | **`LString`** is registered by the locale script. Its argument is a string holding a Lua expression, `L("…")`, around the text in the source language (Chinese). The constructor parses it (`ks.lstring_check`, `ks.llex_parse`), and the text is translated through the locale cache, whose main table maps source text to its translation (175,415 entries in `locale_en`). | **Inferred**; the argument's form **Observed** | `data/core/share/locale.luac` (`locale_traversal`); TS |
| 2.7 | **`Float4`**'s argument is a table of four floats (all 23,408 in `skill`); `CreateFloat4` comes from the design environment. | **Observed** (shape); the constructor **Inferred** | TS; `data/core/share/design_env.luac` |
| 2.8 | **Defaults.** Many rows carry an `__index` field that refers to one shared table of column defaults (every `items` row does, and that table has 66 fields), so a row stores only what differs. This matches the `.tab` files' third line (2.9). | **Observed** (the field); its use as defaults **Inferred** | TS, TT |
| 2.9 | **The `.tab` text format.** UTF-8 with a byte-order mark, CRLF line ends, tab-separated. Line 1 names the columns, line 2 describes them (in Chinese), line 3 gives per-column defaults (often empty), and the rows follow. Checked on three files: `combat/combat_level.tab` (2 columns), `present/skin.tab` (7) and `errors.tab` (8). How the native `tab_file_load` treats quoting or escapes wasn't read. | **Observed** (three files); the rest **Unknown** | TT |

## 3. The sheets

`all_sheets` is one table of 339 sheets keyed by name: items, skills, NPCs, quests, maps, shops, guilds, housing, dungeons, PvP and more (`TS` lists them). A sheet is a table of rows, keyed by integer id or held in its array part, and each row is a table of named fields. The five that M2 dumps:

| # | What | Sheet | Rows | Fields | Some of the fields | Tag | Evidence |
|---|---|---|---|---|---|---|---|
| 3.1 | Items | `items` | 47,112, keyed by id | 118 | `name` and `desc` (`LString`), `level_restriction`, `buy_price`, `max_stack`, `cd`, `icon_id`, `model_id`, `effects` (a table), `__index` | **Observed** | TS |
| 3.2 | Skills | `skill` | 9,177 keyed by id, and 2,934 in the array part | 185 | including `aim_offset_male` and `aim_offset_female` (`Float4`) | **Observed** | TS |
| 3.3 | NPCs and monsters | `npcs` | 855 keyed by id, and 21,109 in the array part (19,167 rows, the rest `nil`) | 21 | `ID`, `name`, `npc_title`, `rank`, `race`, `category` (an editor folder path), `templ_model_id`, `templ_org`, `templ_revive_cd`, `templ_quest1` | **Observed** | TS |
| 3.4 | The experience curve | `upgrade` | 109 rows in a 145-entry array | 2 | `ID`, `experience` | **Observed** (shape); that it is the player's level-up curve **Inferred** | TS |
| 3.5 | Maps | `maps` | 214 keyed by id, and 33 in the array part | 12 | `ID`, `name` and `desc` (`LString`), `resoureName`, `mapResoure`, `loading`, `far_clip`, `hmap_load_range`, `view_range_factor` | **Observed** | TS |

**M2 update (Task 13).** `archive dump` writes the five as JSON lines under `re/`: items 47,112 rows, skills 11,704 (the array part's non-`nil` entries included), monsters 20,022, the experience curve 109, maps 242. Every line parses as JSON, each row's defaults are merged in (`items` rows carry 63 to 79 fields), and none of the five holds a cycle. **Observed** (TU).

**Inferred:**
- `combat_level` is an array of 181 integers, from `data/tables/combat/combat_level.tab` (columns `lv` and `combat_level`): a combat level for each character level, beside `upgrade`'s experience.
- `level_define`, `lv_templates` and the `player_*_lv` sheets hold other level data.

**Unknown:**
- **Monsters.** What marks an `npcs` row as a monster. No field does on its own; `fight_npc_prop`, `fightnpc_level`, `fightnpc_list` and the `monster_*` sheets hold combat data.
- **Shared tables.** How much of the cache's sharing (2.2) is defaults (2.8) and how much other shared values.
- **The diff patch.** Whether a `table_diff.patch` ever shipped: `_index` names none.

## Evidence index

| Key | Path or address | What it holds | Findings |
|---|---|---|---|
| TP | `re/m2/archive/paths.tsv` (`archive paths`, M2 plan Task 12) | Every `_index` path with its record | 1.1, 1.2 |
| TE | `re/m1/list.tsv` (M1's listing of the exe's embedded file system); the exe's strings (Ghidra string search for `tables/`) | Table paths the client names | 1.1 |
| TD | `re/m2/tables/*.new_cache` (probe `re/m2/probes/cache_extract.rs`, run as a `fenji-data` example; `re/m2/oodle-helper-run3.log`, 668 blocks decoded) | The decoded caches | 1.2, 1.5, 2.1, 2.3 |
| TS | `re/m2/tables-survey.txt`, `re/m2/tables-sheets5.txt`, `re/m2/tables-sheets-lv.txt` (probes `re/m2/probes/survey.py`, `re/m2/probes/sheets.py`) | Sheet names, row counts, field names and types, magic counts | 2.3, 2.5–2.8, §3 |
| TT | `re/m2/tables/combat_level.tab`, `skin.tab`, `errors.tab` (decoded as TD) | The `.tab` text format | 1.5, 2.8, 2.9 |
| TU | `re/m2/dump/*.jsonl` (`archive dump --table <name> --out re/m2/dump`), `re/m2/dump-summary.txt`, `re/m2/dump-check.txt` | The five sheets dumped, and the check that every line parses | §3 |
| FUN_1400D56D0, FUN_1400D4AC0, FUN_1400D53D0 | The pinned exe | The unpacker's header and values; the packer's compression | §2 |
