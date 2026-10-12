# M2: the client's tables

The client doesn't ship its design tables (items, skills, NPCs, quests, maps and the rest) as `.tab` files. It ships them compiled into one cache, `data/caches/all_tables.new_cache`. The cache is a single value in the client's own pickle format ([`../protocol/payloads.md`](../protocol/payloads.md)), with two features the network messages don't use: tables referenced from several places, and "magic" objects rebuilt by constructors. With the Oodle helper (M2 plan Task 11) and `_index` (Task 12), the whole cache decodes, and it holds 339 sheets, items, skills, NPCs, the experience curve and maps among them. The few `.tab` files that do ship are UTF-8 text with three header lines.

**Method.**
- **Build:** the pinned build 10015727, `bin64/gujianol.exe` SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678` ([`pinned-build.md`](pinned-build.md)).
- **Ghidra:** the shared project, read-only (no renames, comments, scripts or debugger), for the unpacker and the magic constructors. Each function cited was then read through REA too ([`README.md`](README.md#the-norm)); the Evidence IDs are in the [Evidence index](#rea-evidence-ids).
- **Scripts:** the client's own, decompiled locally in M1. Script evidence cites the script's engine path.
- **Decoding:** `archive paths` found the files; the Oodle helper decoded their blocks inside the sandbox; `archive extract --key` writes any one of them; local probes (Python's standard library, and a `fenji-data` example) read the decoded caches and printed names and counts only.
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
| 1.1 | `_index` names 150 `.tab` files: under `data/tables/present`, `cutscene`, `locale` and `combat`, one at `data/tables/errors.tab`, and two in each of 61 map directories under `asset/maps`. (`data/tables/player` holds three `.txt` files.) None of them holds items, skills, NPCs, quests or maps, although the scripts name such files (`data/tables/quest/quest_main.tab` in `quest_chapter`, `data/tables/maps/map.tab` in `quest_route`), as do the exe's strings (`data/tables/item/shop/…`). Neither `_index` nor the exe's embedded file system holds those paths. | **Observed** | TP, TE; `data/core/modules/quest/quest_chapter.luac`, `data/core/modules/questinfo/quest_route.luac` |
| 1.2 | `_index` also names `data/caches/*.new_cache`: `all_tables` (13,025,329 bytes stored, 605 Oodle blocks, 316,758,404 bytes raw), `combat_tables`, `prop_tables`, `protocol_ver`, and `locale_<language>` for `de`, `en`, `fr`, `kr`, `ru_RU`, `vi` and `zh_MYSG`. | **Observed** | TP, TD |
| 1.3 | `content_system` loads `data/caches/all_tables.new_cache` through the scripts' virtual file system, unpacks it with `ks.pickle_unpack`, and keeps the result as the global `all_sheets`. If `data/caches/table_diff.patch` exists, it applies that file's entries (each a `key_path`, a `key_list` and a `value`) on top. | **Inferred** | `data/core/client/content_system.luac` |
| 1.4 | The other caches have their own loaders: the locale script reads the locale caches, the actor class registry reads `prop_tables` and `combat_tables`, and the enums script reads `protocol_ver`. | **Inferred** | `data/core/share/locale.luac`, `data/core/xobject/client_actor_class_register.luac`, `data/core/share/enums.luac` |
| 1.5 | So the design tables reach the client as one compiled cache, and the `.tab` files that ship are the few the client reads directly. That the cache is compiled from `.tab` sources is our reading: `combat_level.tab`'s 181 rows match the cache's 181-entry `combat_level` sheet (§3). | **Inferred** | TD, TT |
| 1.6 | `archive extract --key` wrote six of these files (`all_tables`, `locale_en` and `prop_tables`, and the three `.tab` files of 2.9), each one byte for byte what the helper's first decode gave. Each file's SHA-1 is its key, so an Oodle file is keyed by its decoded content, as a stored file is ([`m1-archives.md`](m1-archives.md)). | **Observed** | TX |

## 2. The cache format

A cache is the pickle of [`payloads.md`](../protocol/payloads.md) P.1–P.2, plus the forms below.

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | **Compression.** The small caches (`combat_tables`, `prop_tables`, `protocol_ver`) start with `FF`: LZ4-compressed, as P.1 and P.5 describe. `all_tables` (316,758,404 bytes) and `locale_en` (31,355,169 bytes, the one locale cache decoded) start with `81`: uncompressed (their Oodle blobs compress them in the archive). Why, if the client's own packer made them: it compresses only when LZ4's worst case for the packet (its size, plus 16, plus a 255th of it) fits in 1 MiB. | **Observed** (the first bytes and sizes); the reason **Inferred** | TD; FUN_1400D53D0 |
| 2.2 | **Table references.** The header's count byte is `0x81`, so one value follows a 24-bit big-endian count N (P.1). The unpacker numbers the first N tables in the order their `0B` tag is read, before their contents, and tags `0E`, `0F` and `10` carry an 8-, 16- or 32-bit index below the number registered so far ("bad tref" otherwise). Such a tag stands for that same table, so a table can appear in several places, an ancestor included. | **Inferred** | FUN_1400D56D0, FUN_1400D4AC0 (cases `0B`, `0E`–`10`) |
| 2.3 | `all_tables` declares 3,749,858 referable tables and registers all of them; `locale_en` declares 29. Both decode with 2.2 and end exactly on their last byte. | **Observed** | TD, TS |
| 2.4 | **Magic objects.** Tag `0D` is followed by a name value and then one argument value. The unpacker looks the name up in the table the Lua registry holds at integer key 15 ("bad magic logic" if there is no table, "unknown magic: %s" if the name gives no function), calls that function with the argument, and the result takes the object's place. A `nil` name means the next value stands for itself. Two things fill that table: the scripts' `ks.pickle_unpack_magic_setup(name, function)` (FUN_1400D5890, bound by FUN_1400D7660), and the engine's own FUN_1400E4550, which takes a C name and a C function. | **Inferred** | FUN_1400D4AC0 (case `0D`), FUN_1400D5890, FUN_1400D7660, FUN_1400E4550 |
| 2.5 | `all_tables` holds two kinds: `LString` (354,404) and `Float4` (23,499). | **Observed** | TS |
| 2.6 | **`LString`** is registered by the locale script through `ks.pickle_unpack_magic_setup`, beside `Locale`, which `all_tables` doesn't use. Its argument is a string holding a Lua expression: `L("…")` around the text in the source language (Chinese) in 354,148 of them, and another expression (a call around `L`, or `L` with a second argument) in the other 256. The constructor parses it (`ks.lstring_check`, `ks.llex_parse`), and the text is translated through the locale cache, whose main table maps source text to its translation (175,415 entries in `locale_en`). | **Inferred**; the arguments' forms **Observed** | `data/core/share/locale.luac` (`locale_traversal`); TS, TR |
| 2.7 | **`Float4`** is registered by the engine itself, not by a script: FUN_1400E8630 gives FUN_1400E8140 to FUN_1400E4550 under the name `Float4`, and also sets it as `ks.float4_new`. The design environment's `CreateFloat4` (and `CreatePos`) is `ks.float4_new` under another name; the unpacker never looks there. All 23,499 arguments are tables of four numbers, 23,408 of them in `skill`; the constructor also takes four numbers or another `Float4`. | **Inferred** (the registration and the constructor); the arguments **Observed** | FUN_1400E8630, FUN_1400E8140, FUN_1400E4550; `data/core/share/design_env.luac`; TS, TR |
| 2.8 | **`__index`.** Every `items` row (47,112) has an `__index` field, and each one refers to the row itself. No other table in the cache has an `__index` field, so no row takes defaults from another table. The self-reference is Lua's class idiom: a table that sets a row as its metatable reads the row's fields through `__index`. Where the client does that wasn't traced. | **Observed** (the fields); the idiom **Inferred** | TR |
| 2.9 | **The `.tab` text format.** UTF-8 with a byte-order mark, CRLF line ends, tab-separated. Line 1 names the columns, line 2 describes them (in Chinese), line 3 gives per-column defaults (often empty), and the rows follow. Checked on three files: `combat/combat_level.tab` (2 columns), `present/skin.tab` (7) and `errors.tab` (8). How the native `tab_file_load` treats quoting or escapes wasn't read. | **Observed** (three files); the rest **Unknown** | TT |

## 3. The sheets

`all_sheets` is one table of 339 sheets keyed by name: items, skills, NPCs, quests, maps, shops, guilds, housing, dungeons, PvP and more (`TS` lists them). A sheet is a table of rows, keyed by integer id or held in its array part, and each row is a table of named fields. The five that M2 dumps, and `new_npcs` beside `npcs`:

| # | What | Sheet | Rows | Fields | Some of the fields | Tag | Evidence |
|---|---|---|---|---|---|---|---|
| 3.1 | Items | `items` | 47,112, keyed by id | 118 | `name` and `desc` (`LString`), `level_restriction`, `buy_price`, `max_stack`, `cd`, `icon_id`, `model_id`, `effects` (a table), `__index` (the row itself, 2.8) | **Observed** | TS, TR |
| 3.2 | Skills | `skill` | 9,177 keyed by id, and 2,934 in the array part | 185 | including `aim_offset_male` and `aim_offset_female` (`Float4`) | **Observed** | TS |
| 3.3 | NPCs and monsters | `npcs` | 855 keyed by id, and 21,109 in the array part (19,167 rows, the rest `nil`) | 21 | `ID`, `name`, `npc_title`, `rank`, `race`, `category` (an editor folder path), `templ_model_id`, `templ_org`, `templ_revive_cd`, `templ_quest1` | **Observed** | TS |
| 3.4 | The experience curve | `upgrade` | 109 rows in a 145-entry array | 2 | `ID`, `experience` | **Observed** (shape); that it is the player's level-up curve **Inferred**: the NPC module's `GetMaxExpByLv` returns `upgrade[lv].experience`, and the experience bar uses it | TS; `data/core/modules/npc/npc.luac`, `data/interface/addons/ExpBar/ExpBar.luac` |
| 3.5 | Maps | `maps` | 214 keyed by id, and 33 in the array part, 5 of them `nil` | 12 | `ID`, `name` and `desc` (`LString`), `resoureName`, `mapResoure`, `loading`, `far_clip`, `hmap_load_range`, `view_range_factor` | **Observed** | TS, TR |
| 3.6 | NPC properties | `new_npcs` | 20,022: the same ids as `npcs`'s rows, each a table of its own (none shared with `npcs`) | 40 | `ID`, `prop_npc_type`, `prop_npc_rank`, `prop_npc_org`, `prop_npc_race`, `prop_base_out_of_combat`, `prop_npc_show_bar`, `prop_npc_selectable`; with `npcs` it shares only `ID` and `prop_npc_progress` | **Observed** | TR |

**M2 update (Task 13).** `archive dump` writes the five as JSON lines under `re/`: items 47,112 rows, skills 11,704 (the array part's non-`nil` entries included), monsters 20,022, the experience curve 109, maps 242 (5 of the 247 entries are `nil`). Every line parses as JSON, each row holds its own fields (`items` rows 63 to 79, with the self-referring `__index` left out), and none of the five holds a cycle. The dump's `monsters` is `npcs`, the sheet that names each NPC; `archive dump --table new_npcs` writes the same 20,022 ids' properties. **Observed** (TU).

**Inferred:**
- `combat_level` is an array of 181 integers, from `data/tables/combat/combat_level.tab` (columns `lv` and `combat_level`): a combat level for each character level, beside `upgrade`'s experience.
- `level_define`, `lv_templates` and the `player_*_lv` sheets hold other level data.
- `new_npcs` holds the NPCs' actor properties: the actor manager's `prepare_npc_base_props` fills an NPC actor's (actor type 2) properties from it, and the content system and the action events read it too. `npcs` is what the NPC module reads for names, titles, quests and models (`data/core/client/actor_manager.luac`, `content_system.luac`, `action_event.luac`; `data/core/modules/npc/npc.luac`).

**Unknown:**
- **Monsters.** What marks an NPC as a monster. The candidates are `npcs`'s `rank`, `race` and `category` and `new_npcs`'s `prop_npc_type` and `prop_npc_rank`; `fight_npc_prop`, `fightnpc_level`, `fightnpc_list` and the `monster_*` sheets hold combat data.
- **Shared tables.** What the cache's references (2.2) point at, beyond each `items` row's `__index` naming itself (2.8).
- **The diff patch.** Whether a `table_diff.patch` ever shipped: `_index` names none.

## Evidence index

| Key | Path or address | What it holds | Findings |
|---|---|---|---|
| TP | `re/m2/archive/paths.tsv` (`archive paths`, M2 plan Task 12) | Every `_index` path with its record | 1.1, 1.2 |
| TE | `re/m1/list.tsv` (M1's listing of the exe's embedded file system); the exe's strings (Ghidra string search for `tables/`) | Table paths the client names | 1.1 |
| TD | `re/m2/tables/*.new_cache` (probe `re/m2/probes/cache_extract.rs`, run as a `fenji-data` example; `re/m2/oodle-helper-run3.log`, 668 blocks decoded); `archive extract --key` now writes the same files (TX) | The decoded caches | 1.2, 1.5, 2.1, 2.3 |
| TS | `re/m2/tables-survey.txt`, `re/m2/tables-sheets5.txt`, `re/m2/tables-sheets-lv.txt` (probes `re/m2/probes/survey.py`, `re/m2/probes/sheets.py`) | Sheet names, row counts, field names and types, magic counts | 2.3, 2.5–2.7, §3 |
| TR | `re/m2/tables-rows-check.txt` (probe `re/m2/probes/check_cache_rows.rs`, run as a `fenji-data` example on `all_tables`) | Every `__index` field and what it names; each sheet's rows and field names; `npcs` against `new_npcs`; `maps`' `nil` entries; the `LString` and `Float4` arguments' forms | 2.6–2.8, §3 |
| TT | `re/m2/tables/combat_level.tab`, `skin.tab`, `errors.tab` (decoded as TD) | The `.tab` text format | 1.5, 2.9 |
| TU | `re/m2/dump/*.jsonl` (`archive dump --table <name> --out re/m2/dump`), `re/m2/dump-summary.txt`, `re/m2/dump-check.txt`; `re/m2/dump-fix/` (the five dumped again after Task 13's review, byte for byte the same, and `new_npcs`) | The sheets dumped, and the check that every line parses | §3 |
| TX | `re/m2/extract-check.txt` (`archive extract --key <key> --out re/m2/extract-check` for six files, each compared with TD and its SHA-1 with its key) | Files written by key | 1.6 |
| FUN_1400D56D0, FUN_1400D4AC0, FUN_1400D53D0, FUN_1400D5890, FUN_1400D7660, FUN_1400E4550, FUN_1400E8630, FUN_1400E8140 | The pinned exe (Ghidra; REA IDs below) | The unpacker's header and values; the packer's compression; the two ways magic constructors are registered; `Float4`'s | §2 |

### REA Evidence IDs

Read through REA 6.3.0 with Ghidra 12.1.4 on the pinned exe, in one session on 2026-10-12, after the findings had been written from the shared Ghidra project; each reading shows the function in the role the finding gives it. An ID covers its call exactly as sent, so anyone can check it by repeating the call ([`README.md`](README.md#how-to-cite)). FUN_1400D4AC0, FUN_1400D56D0, FUN_1400D53D0 and FUN_1400D7660 gave the same IDs as in an earlier session.

| Function | Cited in | Call | Evidence ID |
|---|---|---|---|
| FUN_1400D4AC0 | 2.2, 2.4 | `procedure_pseudo_code {procedure: "0x1400d4ac0"}` | `ev_1e76a59eba406f609264ca691ad3bccb2745968beb2eee7d29ff0d7553d89644` |
| FUN_1400D53D0 | 2.1 | `procedure_pseudo_code {procedure: "0x1400d53d0"}` | `ev_6a81a461fcb184bf11b2740aea8c26a299321f2d5ee976f4d08c3b75eeae3856` |
| FUN_1400D56D0 | 2.2 | `procedure_pseudo_code {procedure: "0x1400d56d0"}` | `ev_f590e6bd21bf27155144c8121d799eb9f37779edb6c824dbe6e452a187e235c4` |
| FUN_1400D5890 | 2.4 | `procedure_pseudo_code {procedure: "0x1400d5890"}` | `ev_814a88e1418f5d7399f3461e1dced0956609afe4fccc685108f895d910f63ccf` |
| FUN_1400D7660 | 2.4 | `procedure_pseudo_code {procedure: "0x1400d7660"}` | `ev_892872725873074678d85f177a260fd4f7bf23dcdbccaf4ad4893f3504196f0e` |
| FUN_1400E4550 | 2.4, 2.7 | `procedure_pseudo_code {procedure: "0x1400e4550"}` | `ev_daa93610799aa205b9a71c4a4c0dbbd062cb363c79bfc98e45a41e0ee78f24aa` |
| FUN_1400E8140 | 2.7 | `procedure_pseudo_code {procedure: "0x1400e8140"}` | `ev_6814bdb87f42bf7d3e6d71e9ad6c33490d8fae722a557df97d976c9c79e793dd` |
| FUN_1400E8630 | 2.7 | `procedure_pseudo_code {procedure: "0x1400e8630"}` | `ev_6660a1d3b14a5f52735a168d3fbece0ba5d05554dd132ec4d2dda0838cf21e4a` |
