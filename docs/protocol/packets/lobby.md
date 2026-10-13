# Lobby: the character list, creation and deletion

In `WORK` the client waits on its title screen. A key press sends 292, and the server answers with 228: the pid to pre-select, the account's characters and a table of account counters. An empty list opens character creation. Otherwise the select screen opens with `9 + role_num` places. Creating sends one table (304) and waits for 628 or 270. Deleting takes a typed confirmation, sends 166 and expects 84, and then the client asks for the list again. Entering the world (10) is M4's work, so M3 refuses it with 270. All of these are script messages on the one gate connection, and the client names each one in its own protocol table. This document gives the evidence behind the values that `fenji_proto::lobby` uses.

**Method.** This reading comes from the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`). It used three sources, all read locally and read-only:
- the client's own scripts, disassembled and decompiled by `luatool` (`docs/re/m1-script-format.md`);
- the client's own caches and layouts: the protocol table, the English strings, the design tables, the `errors` sheet (`archive dump --table errors`) and the add-ons' `.xml` layouts;
- for the close message, the pinned exe as [`opcodes.md`](../opcodes.md) and [`transport.md`](../transport.md) read it.

A first agent read the scripts and wrote notes (L1). A second agent checked the five claims the merged code depends on against the bytecode (L2): 228's arguments, the actor fields, the create request, 84's values and the error path. It agreed with all five, recorded three refinements, and wrote this document. Nothing was run for §1–§13: no client, no debugger, no script in Ghidra. §14 records the M3 client runs that came later. Both agents have not read any source rejected in [`../../re/sources.md`](../../re/sources.md). Tags follow [`../../re/spike-0.md`](../../re/spike-0.md): **Observed** (literally in a file: a constant, a string, a bytecode operand, a table value, a layout attribute), **Inferred** (our reading of the code), **Unknown**. Script evidence cites the engine path and the function path in its disassembly (such as `0.32`). Where the decompiled text and the bytecode differ, the bytecode wins. Native evidence cites addresses in the pinned exe. `errors` rows are cited by `literal` and `ID`. English texts are the client's own `locale_en` strings. Evidence keys resolve in the [Evidence index](#evidence-index).

## 1. Which copy loads

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 1.1 | `ks.get_locale()` returns a code, a name, a language and the locale's `load.config`. The name is the setting `--server_locale`, or `gameforge` when it is unset. The `gameforge` entry has code 3, `is_common = true` and the default language `en`. | **Observed** (constants), **Inferred** (flow) | S17 (`locale_config.luac` `0`) |
| 1.2 | `data/locale/gameforge/load.config` names `common` as its `[default]`. | **Observed** | D5 |
| 1.3 | The module loader rewrites a module's path. `load_localed_module_spec` (`module_system.luac` `0.22`) tries the path with `^data/` replaced by `data/locale/<name>/`, then by `data/locale/<default>/`, then the path itself, and takes the first that exists. So for the Western client a `gameforge` copy wins, then a `common` copy, then the base file. Module specs (`0.20`) and add-ons load through it. | **Inferred** | S17; L1 0.3 |
| 1.4 | `data/locale/gameforge/` holds only `const_base`, `utils` and `scripts/test`. `data/locale/common/` holds whole copies of the ActorSelect and ActorCreate add-ons (scripts and layouts) and of `core/modules/pregame`. So the `common` copies are the ones that load. The ActorSelect, ActorCreate and pregame citations below are to them; Login's (S5) is to the base copy. | **Observed** (file lists), **Inferred** (load) | S6–S14 |
| 1.5 | `SYSTEM.GetServerLocale()` returns the constant 3, and `SYSTEM.IsCommonLocale()` returns `true`. The lobby's scripts branch on them; Login, for example, picks its logo sound and its music by `IsCommonLocale()`. | **Observed** | S17 (`system.luac` `0.98`, `0.100`); S5 `0.10` |

## 2. Messages

All lobby messages are script messages on the one gate connection. The client sends them as `CTS_PROTOCOL_GATE_SCRIPT` (type 0xFF), and the server as `STC_PROTOCOL_SCRIPT` (type 0). Each body is the pickle of the message id and its arguments ([`../payloads.md`](../payloads.md)). In both tables, ids and names are **Observed** (D1, and the literal ids in the scripts), arguments are **Observed** in the senders' and handlers' bytecode, and effects are **Inferred** unless a row says otherwise.

### Client to server

| Id | Name (D1) | Arguments | Sent by | What the server does in M3 |
|---|---|---|---|---|
| 292 | `PROTOC2S_ROLE_LIST` | none | `NET.QueryActors` (S1 `0.48`), after 370 and 496 | answers with 228 (§3) |
| 370 | `PROTOC2G_HOTPATCH_INIT` | `hotpatch_revision`, 0 at start | `hotpatch_pull` (S2 `0.4`) | nothing: the reply, 243, is never sent |
| 496 | `PROTOC2G_GM_SCRIPTS` | none; sent only when `is_shipping_client()` | `hotpatch_pull` (S2 `0.4`) | nothing: the reply, 216, is never sent |
| 402 | `PROTOC2G_SERVER_LIST` | none | the 414 handler (S1 `0.29`) | nothing; 495 is optional (§11) |
| 304 | `PROTOC2S_ROLE_CREATE` | one table (§5) | `NET.CreateActor` (S1 `0.50`), from the create panel (S12 `0.1`) | 628 or 270 (§6, §8) |
| 166 | `PROTOC2S_ROLE_DELETE` | pid | `NET.DeleteActor` (S1 `0.51`), from the delete dialog (S8 `0.0`) | 84 or 270 (§7) |
| 10 | `PROTOC2S_ROLE_SELECT` | pid | `NET.SelectActor` (S1 `0.49`), through `UIMain.RequestEnterScene` (S23 `0.17`) | 270 (§10) |
| 26 | `PROTOC2G_ROLE_RENAME` | two values (not read) | `NET.RenameActor` (S1 `0.52`) | nothing (§11) |
| 454 | `PROTOC2G_ROLE_REBORN` | three values (not read) | `NET.RebornActor` (S1 `0.53`) | nothing (§11) |
| 321 | `PROTOC2G_ROLE_SLOT` | pid, slot, then nil or the other pid | `NET.ChangeActorSlot` (S1 `0.54`): the list builder renumbers with two values, and the up and down buttons swap with three (S6 `0.3`, `0.5`) | nothing: 509's handler only logs |
| 392 | `PROTOC2S_CLIENT_EXIT` | none | `SYSTEM.RequestQuitGame` (S17, `system.luac` `0.5`) | closes the connection |

### Server to client

| Id | Name (D1) | Arguments | Handler | Effect | Fenji |
|---|---|---|---|---|---|
| 228 | `PROTOG2C_ROLE_LIST` | last pid, list, extra | S1 `0.32` | keeps a copy of the list, takes `billing_gap`, fires `EVENT_QUERY_ACTORS` (§3) | for every 292 |
| 628 | `PROTOG2C_ROLE_CREATE` | actor | S1 `0.33` | adds the actor to the list if it has a `pid`, fires `EVENT_CREATE_ACTOR_RESULT` (§6) | for an accepted 304 |
| 84 | `PROTOG2C_ROLE_DELETE` | pid, time in ms, level | S1 `0.34` | fires `EVENT_DELETE_ACTOR_RESULT` (§7) | for an accepted 166 |
| 270 | `PROTOS2C_ERROR_CODE` | code, then format arguments | S16 `0.8` | prints the code's text and fires `ERR.EVENT_SERVER_ERROR` (§8) | for each refusal |
| 414 | `PROTOG2C_SET_NET_CONNECTED` | time-zone offset, billing table, user id | S1 `0.29` | moves the client to `WORK` and sends 402 ([`../transport.md`](../transport.md) 10.3) | once, after `ok` |
| 495 | `PROTOG2C_SERVER_LIST` | server list, group list, server map, homeland groups, user server id, user global id | S1 `0.30` | stores them, sets the window title, fires `EVENT_NET_CONNECTED`; in the lobby it only sets the server-name label (L1 Q1) | not sent |
| 509 | `PROTOG2C_ROLE_SLOT` | two values | S1 `0.43` | logs them | not sent |
| 368 | `PROTOS2C_PLATFORM_BILLING_REMAIN` | six values | S1 `0.42` | updates the billing fields and gaps, fires `EVENT_NET_CONNECTED` | not sent |
| 786 | `PROTOG2C_QUEUE_WAIT_TIME` | two values | S1 `0.31` | fires `EVENT_QUEUE`, which the login queue's display shows (S5 `0.4`) | not sent: no queue |
| 243 | `PROTOG2C_HOTPATCH_INIT` | revision, files | S2 `0.2` | replaces scripts and layouts (2.3) | never |
| 216 | `PROTOG2C_GM_SCRIPTS` | files | S2 `0.3` | replaces scripts and layouts (2.3) | never |
| 556 | `PROTOS2C_GM_SCRIPT_INVOKE` | a chunk | S3 `0.15`, then `0.14` | runs Lua bytecode (2.3) | never |
| 627 | `PROTOG2C_SHEET_HOTPATCH` | patches | S21 `0.13` | patches the content tables (2.3) | never |
| 729 | `PROTOG2C_USER_DETAIL` | two values | S1 `0.38` | writes a file (2.3) | never |
| 720 | `PROTOS2C_CUSTOMER_FEEDBACK_URL` | URL | S16 `0.9` | opens it in the default browser (2.3) | never |
| 598 | `PROTOS2C_GAMEFORGE_SEND_RECHARGE_URL` | URL | S1 `0.46` | opens it in the Gameforge overlay (2.3) | never |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | **Names.** The client's protocol table, `data/caches/protocol_ver.new_cache`, is `{c2s = {[id] = name}, s2c = {[id] = name}, ver = 3861871408}`, and `ver` is the hello's `K`. `enums.luac` loads it with `ks.pickle_unpack` and gives each `PROTO*` name its id, unless a source `enum_protocols.lua` exists or `--cmd` is set; that developer path numbers the names itself. Every literal id in `net.luac`'s handlers and senders has the name the table gives it, so the shipping client takes the table's path. This also names 402, which [`opcodes.md`](../opcodes.md) had left open. | **Observed** (table, literals), **Inferred** (the path taken) | D1; S20 `0`; S1 `0` |
| 2.2 | **Handlers and listeners.** `net_manager.luac` `0.15` hands a type-0 message to the handler registered for its id (`SetStub`; `net.luac` `0` registers its handlers at [118]–[155]), with the remaining values as arguments. The handlers fire events. `event_system_fire_event(event, …)` calls each listener as `callback(event, owner or scope, custom, …)` under `__script_system_trace_pcall`, and `RegisterEvent` registers the callback itself. So a listener's first fired value is its fourth parameter. An error in a listener is caught and logged ("Errors on fire event"), and the rest of that listener is skipped. | **Observed** (bytecode), **Inferred** (meaning) | S3 `0.15`; S4 |
| 2.3 | **Never sent.** Seven server messages act on the player's machine beyond the game screen, so Fenji never sends them. `fenji_proto::packet::NEVER_SEND` lists them, and `script_body`, through which every script message passes, refuses them:<br>1. 556 runs Lua bytecode. `net_manager.luac` `0.15` passes it to `0.14`, which `load`s the chunk in binary mode (`"b"`), runs it, and answers with 278 (`PROTOC2S_GM_SCRIPT_REPORT`) and the chunk's MD5 ([`../transport.md`](../transport.md) 10.7).<br>2. 243 and 216 replace scripts and layouts. Both store `{path → content}` in `hotpatch_contents` (`0.2`, `0.3`). The module loader, the design-script loader, the add-on manager and the UI layout loader consult that store before the archive (`hotpatch_fetch_content`, `0.5`). 243 also applies the files at once: `scripts/…` files through `design_script_import`, any other file through `game_system_require_reload()`.<br>3. 627 patches tables: `content_system.luac` `0.13` writes values into the client's content tables, unlocking locked ones.<br>4. 729 writes a file to the player's disk: `net.luac` `0.38` opens `"%s/../uinfo_%d.xml"` under `daemon_work_path` with mode `"w"`.<br>5. 720 and 598 open URLs: `err.luac` `0.9` calls `SYSTEM.OpenUrlWithDefaultBrowser`, and `net.luac` `0.46` calls `net_manager_gameforge_open_url_overlay`. | **Observed** (calls, constants), **Inferred** (effects) | S1, S2, S3, S16, S21; L2 |
| 2.4 | **Also not sent in M3.** The scene and world messages (772 `PROTOS2C_ACTOR_LOAD_SCENE`, 92 `PROTOS2C_ACTOR_RELOGIN`, 330 `PROTOS2C_ACTOR_LOGOUT`, 394 `PROTOS2C_ACTOR_KICKOUT`) and the scene queue (605, 728) are M4's; 605's handler asserts its arguments. 228 and 628 are never sent unasked: Login acts on any 228 at once and changes screens (3.11), and a 628 closes the create screen (6.2). | **Inferred** | D1; S22; L1 Q12 |

## 3. Title screen to list

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 3.1 | **"Press Any Key".** In `WORK`, Login's 100 ms `login` timer shows the `Prompt` label, "任意键开始游戏" ("Press Any Key"), as soon as `NET.GetStatus()` returns 3. Nothing waits for a server message first: in M2 the prompt appeared with no 495 sent. | **Observed** (the label; the M2 run), **Inferred** (flow) | S5 `0.6`; D5 (`Login.xml` line 71); D2; [`../transport.md`](../transport.md) M2R4B |
| 3.2 | **The key press.** `OnGlobalKeyUp` calls `NET.QueryActors()` only when the status is 3, `LoginFrame` is visible, no global alert is open, and none of Login's own gates is set (the logo video, the protocol dialog, a transition under way). It then sets the gate `L9_1`, so further presses do nothing until a 228 arrives. | **Inferred** | S5 `0.24` |
| 3.3 | **370, 496, 292.** `QueryActors` calls `hotpatch_pull`. That sends 370 with `hotpatch_revision` (0 unless a 243 has changed it) and, when `is_shipping_client()`, 496 with no arguments. Then `QueryActors` sends 292 with no arguments. So the server sees 370, 496 and 292 in that order, all of type 0xFF, and only 292 needs an answer. Every M3 run that asked for the list saw exactly this order, the three read by the server within 0.3 ms of each other (server log times, not send times): after a click on the title (m3-l2, m3-l3a), after Return (m3-l2, m3-l3b, m3-l4, m3-l5) and in the refresh after a delete (m3-l5). One Return on the title sent nothing (m3-l3a, §14). | **Observed** (bytecode; the order on the wire) | S1 `0.48`; S2 `0.4`; M3L2–M3L5 |
| 3.4 | **228's handler.** It keeps `deepcopy(list)` as `query_actors_list`. It calls `OnBillingGapsUpdate(extra.billing_gap)` when `extra` has one; otherwise it sets `server_info.billing_gap` to `{}` if that is unset. Then it fires `EVENT_QUERY_ACTORS(last_pid, list, extra)` with the original list. Two listeners act on it: Login's and the select screen's. | **Observed** (constants), **Inferred** (flow) | S1 `0.32`; S5 `0.10`; S6 `0.10`, `0.12` |
| 3.5 | **The first argument: the pid to pre-select.** Login compares it with each `actor.pid` (`==`) to pick the transit cutscene. `ShowSelectActor` makes it the selection when none is set, and the list builder selects that actor's frame, or the frame in slot 1 when nothing matches. So 0, or a pid that matches no actor, selects slot 1. It must have the same type as `pid`: an integer. | **Inferred** | S5 `0.10`, `0.9`; S6 `0.16`, `0.3` |
| 3.6 | **The second argument: the list.** A Lua sequence of actor tables (§4), and a table even when empty. Login tests `list and next(list)`. A nil list would leave the handler's copy nil, and a later 628's `table.insert` into it would fail. | **Inferred** | S5 `0.9`; S1 `0.32`, `0.33` |
| 3.7 | **The third argument: the account's extras.** The select screen's listener passes all its values on to `SetAccountReNum`, which reads its sixth parameter, `extra`, without a nil check. It takes `rename_num`, `reborn_num`, `change_gender_num` and `role_num`, each `or 0`, into `Account_Rename_Num`, `Account_Reborn_Num`, `Account_Denaturalization_Num` and `Account_Role_Num`, which start at 0. A nil `extra` raises inside that listener, which then skips `UpdataServerTime`. `billing_gap`, a map of "YYYYMM" to milliseconds, is optional, and no screen of a common locale shows it. | **Observed** (bytecode: the listener forwards every value, though the decompiled text shows one), **Inferred** (meaning) | S6 `0.10`, `0`; S10 `0.0`; L1 Q2 |
| 3.8 | **The number of places.** The list builder shows `max(9 + Account_Role_Num, #actors)` entries (the constant 9 added to `Account_Role_Num`, logged as "-------Role max num:"): an `ActorItemFrame` for each actor, then `ActorCreateTemplate` entries up to that number. Only those entries carry a create button. So the select screen offers `9 + role_num` places, and an account with that many characters has no way to create another. | **Observed** (bytecode, layout), **Inferred** (effect) | S6 `0.3`; D5 (`ActorFrame.xml` lines 139, 175) |
| 3.9 | **The transition.** Login first fills in a missing `prop_base_skeleton` from the vocation and gender (§4). If the gate from 3.2 is set, it clears it, animates the logo, and plays transit cutscene 6, 7 or 8 for the pre-selected actor's skeleton 1, 2 or 3 (8 when no actor matches). It runs the next step 3.8 s later (`SetDelayTimer("CS01", 3800, …)`). Otherwise it runs the next step at once. | **Observed** (constants), **Inferred** (flow) | S5 `0.10` |
| 3.10 | **Creation or select.** The next step (`0.9`) opens character creation when the list is empty. It first calls `HideSelectActor(true)`, which clears the select screen's list and selection, then `ShowActorCreate(true, …)`. Otherwise it calls `ShowSelectActor(last_pid or 0, list, …)`. | **Inferred** | S5 `0.9`; S6 `0.17`, `0.16` |
| 3.11 | **Every 228 changes the screen.** Login acts on each `EVENT_QUERY_ACTORS`, asked for or not. The select screen asks for the list again after a delete (§7), and whenever `ShowSelectActor` has no selection or no list. The two listeners run in the order they registered, which wasn't read. Fenji sends the same `role_num` each time, so the order doesn't matter. | **Inferred** | S5 `0.10`; S6 `0.16`; S9 `0.1`; S4 |

## 4. The actor table

Each entry of 228's list, and 628's argument, is one actor table. Its readers are the list builder and the entry frame (S6 `0.3`, S7 `0.1.7`), the model preview (S6 `0.0`, then `ChangeActor` and `pregame_actor.luac` `0.4` and `0.0`, S14), the enter and delete paths (S8 `0.1`, S9 `0.1.0`), the rename and reborn buttons (S10 `0.4`) and Login (S5 `0.10`). "Required" means a missing or bad value raises a Lua error or breaks the flow. The dispatcher catches the error, but the rest of that listener is skipped, which can leave a screen half built or the title screen stuck. The source column follows the M3 spec's D8: (1) omitted, or a constant, where the client tolerates that; (2) derived from the operator's own client files; (3) never committed.

| Field | Type | Used for | Required? | Source (spec D8) | Tag | Evidence |
|---|---|---|---|---|---|---|
| `pid` | integer, not 0 | the frame's identity; the selection (`==`); sent back in 10, 166 and 321; the preview actor's id (`actor_system_create_actor(pid, …)`, where nil and 0 mean "no actor"); the countdown timer's key `"delete_cd" .. pid` | **required** | the server's own, never reused | **Inferred** | S6 `0.3`, `0.0`; S19; S7 `0.1.7` |
| `slot` | integer, 1 to n | the order; a missing or out-of-order slot is renumbered with a 321 | tolerated; a number when present (it is compared with `<`) | the server: the list's order | **Observed** (bytecode) | S6 `0.3` |
| `prop_base_name` | string | the name label, through `tostring` (nil shows "nil") | tolerated | 304 | **Observed** | S7 `0.1.7` |
| `prop_base_lv` | integer, 1 or more | the level text, `max(1, floor(lv or 1))`; 84's threshold test | tolerated (1) | the server: 1 | **Observed** | S7 `0.1.7`; S9 `0.1.0` |
| `prop_base_vocation` | integer, 0–9 | the list icon `tVOCATION_IMAGES_SELECT[v + 1]`, the class name, the model template `v * 10000`, the camera, the pet model | **required**: nil raises (arithmetic on nil); a value outside 0–9 concatenates or indexes nil | 304 | **Observed** (bytecode), **Inferred** (range) | S7 `0.1.7`; S14 `0.0`; L1 Q3 |
| `prop_base_skeleton` | integer, 1–3, one the vocation allows | the model's skeleton and gender, the cutscene, the camera, the select animations | tolerated: when it is missing, Login derives it from the vocation and gender (`get_role_skel`, which can fail only for a vocation row that doesn't exist, already required above); the model defaults it to 1 and the camera and model only compare it; how a character looks without it is **Unknown**. Fenji always sends it | 304 | **Inferred** | S5 `0.10`; S15 `0.43`; S14 `0.0` |
| `prop_base_gender` | 0 or 1 | Login's skeleton fallback and the reborn button only | tolerated when the skeleton is sent | derived from the skeleton, as the client derives it: 1 gives 0, 2 and 3 give 1 | **Observed** | S5 `0.10`; S14 `0.0`; S13 |
| `prop_sys_portrait` | integer, `portrait << 16 \| frame` | the head icon and its frame | tolerated (1638400, which is `25 << 16`) | 304 | **Observed** | S7 `0.1.7` |
| `prop_sys_avatar_feature` | integer, the base face, 1 or 2 | a model property (`or 0`) | tolerated | 304, clamped (5.10) | **Observed** | S14 `0.0` |
| `prop_sys_avatar_feature_face` | string, `v:5;…` | the model's face, with no default | tolerated by Lua; how the model looks without it is **Unknown** | 304, when present | **Observed**; the look **Unknown** | S14 `0.0` |
| `prop_sys_equips` | table, slot to item id | the model's equipment, with no default | tolerated by Lua; with `{}` the select screen showed the character dressed, but not in the outfit shown at creation (m3-l3b, §14) | the constant `{}` in M3 (4.4) | **Observed**; that the dress is the vocation's default **Inferred** | S14 `0.0`; M3L3 |
| `prop_sys_show_equip_head`, `_artifact1`, `_artifact2`, `_chest`, `_wrist`, `_boot`, `_weapon`, `_pendantmoon`, `_pendantstar1` and `_pendantstar2`, and `prop_sys_show_exterior` | boolean | the model's show flags, each `x or false`: a missing flag hides its piece | tolerated | the constant `true` | **Observed** | S14 `0.0` |
| `prop_sys_show_equip_ears`, `_tail` | boolean | `x ~= false`: a missing flag shows its piece | tolerated | omitted | **Observed** | S14 `0.0` |
| `prop_sys_exterior_form` | integer | `x or 1` | tolerated | omitted | **Observed** | S14 `0.0` |
| `prop_sys_pendant_custom_positions` | table | `x or {}` | tolerated | omitted | **Observed** | S14 `0.0`; S6 `0.0` |
| `scene_tid` | integer | the "last scene" label | tolerated | omitted until M4 | **Observed** | S7 `0.1.7` |
| `delete_cd` | integer, ms left | a countdown frame when it is above 0, on the local clock | tolerated; a number when present | omitted: deleting is immediate (spec D9) | **Observed** | S7 `0.1.6`, `0.1.7` |
| `block_cd` | integer, absolute server time in ms | "Blocked until: {1}"; entering is refused when `block_cd > GetServerTimeMS()` | **never sent** (4.3) | omitted | **Observed** (the tests), **Inferred** (the effect) | S7 `0.1.7`; S8 `0.1` |
| `block_reason` | string | the block label (default "Other") | tolerated | omitted | **Observed** | S7 `0.1.7` |
| `force_rename`, `rename_cd_cliend` | boolean, integer | the rename button and its dialog | tolerated | omitted | **Observed** (L1) | S10 `0.4` |
| `enable` | none | the slot builder's own flag | **never sent** (4.2) | omitted | **Inferred** | S6 `0.3` |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.1 | **What is required.** The select screen needs `pid` and `prop_base_vocation`. Every other field has a default, is derived (`prop_base_skeleton`, from the vocation and gender) or is skipped when missing, though missing show flags hide the outfit. | **Inferred** | the table above; L2 claim 2 |
| 4.2 | **`enable` is never sent.** The list builder runs `#list` rounds. Each round picks, among the actors without `enable`, the one with the smallest numeric `slot`, or else the first one with no `slot`, and marks it `enable = true`. Then it walks the ordered list: where `slot` differs from the position, it sends `NET.ChangeActorSlot(pid, position)` (321) and sets `slot`; then it sets `enable = false`. An actor that arrives with `enable` set is never picked and keeps the slot it came with. If that slot is nil, `frames[frame.slot] = frame` raises "table index is nil"; if it is a number, the actor may share a place with a renumbered one. `enable = false` does no harm. | **Observed** (bytecode), **Inferred** (effect) | S6 `0.3` [7]–[75], [173]–[174] |
| 4.3 | **`block_cd` is never sent.** `GetServerTimeMS()` floors `server_time_now`, which starts at 0. It advances only while a character is in the world (`time_manager_update` returns early unless the local actor's id is above 0), and otherwise changes only when a time-sync reply sets it ([`../transport.md`](../transport.md) 10.6). So on the lobby screens any `block_cd` above 0 shows the character as blocked and refuses entry. nil, 0 or a negative value does neither. | **Inferred** | S17 (`system.luac` `0.17`); S18; S7 `0.1.7`; S8 `0.1` |
| 4.4 | **Sources.** The server adds `pid`, `slot` and level 1. The 304 gives the name, vocation, skeleton, portrait, base face and face. The gender is derived from the skeleton, as the client derives it. The show flags are constants. `prop_sys_equips` is an empty table. The outfit's item ids live in the operator's own design tables (`vocation_initequip_templ`, with item ids as `type << 24 \| id`), which would be spec D8's second source; M3 doesn't read them (plan Amendment A, the execution ruling on outfits). Nothing from the client is committed. | **Inferred** | D3; L1 Q4; `crates/fenji-proto/src/lobby.rs` (`Actor::to_table`) |

## 5. The create request

| Key | Type | Value in the client | What the Western screen sends | Present |
|---|---|---|---|---|
| `prop_base_name` | string | `InputName:GetText()` | 1–24 bytes (5.4) | always |
| `prop_base_vocation` | integer | `math.floor(eVocationType)` | 0–7 or 9 (5.7) | always |
| `prop_base_skeleton` | integer | `math.floor(eRoleSkel)` | 1, 2 or 3, one the vocation allows | always |
| `prop_sys_player_voice` | number | `nVoiceSelectID` | an id from the skeleton's voice list (L1 Q4) | when the voice panel set it |
| `prop_sys_avatar_feature` | number | `nBaseFaceSelectID`, the base face | 1 or 2 | when the base-face panel set it |
| `prop_sys_avatar_feature_face` | string | `PREGAME.GetFaceData()`, the face editor's data | `v:5;…` (5.9) | when face editing is enabled |
| `prop_sys_player_tattoo_pos` | integer | the constant 0 | 0 | always |
| `prop_sys_player_birthday` | integer | the constant 0 | 0 | always |
| `prop_base_equips_id` | integer | `math.floor(nEquipsSelectIndex)` | 1 or 2: the two outfit buttons | always |
| `prop_sys_portrait` | integer | `nHeadSelectedID << 16` | a portrait id, shifted | always |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 5.1 | **The fields.** The create panel builds one table with the ten keys above and sends it with `NET.CreateActor` as 304's only argument. `math.floor(nil)` and `nil << 16` raise, so a 304 that leaves the client always has the name, vocation, skeleton, outfit and portrait. A nil value makes no entry, so the voice, base face and face may be missing. | **Observed** (bytecode), **Inferred** (presence) | S12 `0.1` [1]–[17], [71]–[87]; S1 `0.50` |
| 5.2 | **No gender.** The request carries no `prop_base_gender`. The client derives it from the skeleton: 1 gives 0, and 2 or 3 give 1. | **Observed** | S13; S14 `0.0` |
| 5.3 | **The client's checks, in order.** An empty name shows "Please give the character a name!". A space shows "The character name may not contain spaces.": both `string.find` calls test the same one-byte constant, 0x20, so only the ASCII space is refused. A name that `SYSTEM.LegalNameInput` rejects (the client's own word filter) shows "The character name contains forbidden words. Please enter a new name.". Each check returns before anything is sent. | **Observed** (constants, texts), **Inferred** (flow) | S12 `0.1` [22]–[70]; S17 (`system.luac` `0.62`); D2 |
| 5.4 | **Length.** The client's only bound is the input box: `<Input name="InputName" maxBytes="24" …>`, the same in the Western and the base layout. That reads as 24 bytes of UTF-8, 24 ASCII letters; whether the engine counts bytes or characters is **Unknown**. There is no minimum beyond a non-empty name. | **Observed**; the counting **Unknown** | D5 (`FeatureSetting.xml` line 517) |
| 5.5 | **The Western server's rule.** `ERR_ROLE_NAME_ALLCHINESE`'s source text says the name has non-Chinese characters. Its English text was rewritten into the Western server's own rule: "Permissible characters: letters a-z, umlauts, grave/acute accents, cedillas, full stops (except at start/end of name), hyphens, underscores". It names no digits and no spaces. The client states no Western length limit anywhere. | **Observed** (texts), **Inferred** (that this was the Western server's rule) | D4 (`ERR_ROLE_NAME_ALLCHINESE`, -2789480703151742396); D2 |
| 5.6 | **Fenji's name rule.** 2–24 bytes of UTF-8. The letters A–Z in either case; the letters with an umlaut (ä ë ï ö ü ÿ), a grave accent (à è ì ò ù), an acute accent (á é í ó ú ý) or a cedilla (ç), in either case; full stops, but not first or last; hyphens and underscores. Names are unique per server by SQLite's `NOCASE`, which folds ASCII letters only (spec D8). | **Inferred** (from 5.4 and 5.5); the 2-byte minimum is Fenji's choice | `lobby::NAME_BYTES`, `DIACRITICS` |
| 5.7 | **Skeletons for each vocation** (the table below). The design tables' `vocation_skel_templ` and the shared checker's own table agree. The create screen enables a skeleton's button through `VOCATION.GetVocationSkel(v, s)`, which reads `vocation_skel_templ`. `InitVocationSetting` binds button `"V" .. (i + 1)` to vocation i, and the Western layout hides `V9`, vocation 8 (`hide="true"`; the base layout shows it). The design tables' `open_vocation` is `0#1#2#3#4#5#6#7#9`. So the Western screen offers nine vocations, without 8. Fenji accepts exactly the pairs offered and refuses the rest with `ERR_ROLE_VOC_SKELETON_NOT_MATCH`. | **Observed** (tables, layout), **Inferred** (what the screen offers); the rule is Fenji's | D3; S15 `0`; S13 (`VocationSetting.luac` `0.3`, `0.12`); D5 (`VocationSetting.xml` line 226); `lobby::skeleton_allowed` |
| 5.8 | **Portraits.** `vocation_skel_templ` also lists the portraits each vocation and skeleton offers, and the create screen shows the first three. `prop_sys_portrait` carries the chosen one shifted left by 16. Fenji keeps any value from 0 to 2^31 − 1 and doesn't check it against the list. | **Observed** (table), **Inferred** (screen) | D3; L1 Q4 |
| 5.9 | **The face string.** `GetFaceData` returns the native `get_face(model)`, or nothing when face editing isn't enabled. The shared validator shows its format: `;`-separated `key:value` sections of comma-separated integers, starting `v:5`. The real one in run m3-l3b, the creation screen's default face, was 490 bytes: six sections (`v`, `2`, `3`, `4`, `5` and `7`) and a trailing `;`, with section 7's four values in two `|`-separated pairs. So a 304 that carries one is over 128 bytes and arrives LZ4-compressed ([`../payloads.md`](../payloads.md) P.5), as that one did. Other faces' sizes are **Unknown** (native); L1 estimated a few hundred bytes to about 2 KB. | **Inferred** (the format); m3-l3b **Observed** | S14 (`face_control.luac` `0.11`); S15 `0.35`; L1 Q4; M3L3 |
| 5.10 | **What Fenji keeps.** The voice (0–64). The base face, clamped to 1–2 as the shared `check_base_face` does (nil gives 1, below 1 gives 1, above 2 gives 2). The face string, up to 4096 bytes of printable ASCII; an empty string means none. The outfit, clamped to 1–2 by analogy: the screen has two outfit buttons, and no shared check covers it. The portrait. A missing base face or outfit takes 1, and one that isn't a number is refused, as any other malformed field is; a missing portrait takes the select screen's fallback, `25 << 16` (ActorFrame `0.1.7`), since the create screen always sends one (5.1). Numbers may arrive as floats without a fraction. Other keys are ignored. A request that isn't one table, or lacks a name, vocation or skeleton, gets `ERR_ROLE_NAME_ILLEGAL`; no real client sends one. The client doesn't call `check_base_face` before sending 304: it applies it when it loads face data. | **Observed** (`check_base_face`), **Inferred** (the rest) | S15 `0.36`; S14 (`face_control.luac` `0.2`); `lobby::CreateRequest::parse` |

| Vocation | Name (`locale_en`) | Skeletons | On the Western screen |
|---|---|---|---|
| 0 | Spearmaster | 1, 2 | offered |
| 1 | Berserker | 1, 2 | offered |
| 2 | Spellsword | 1, 2 | offered |
| 3 | Bard | 1, 2 | offered |
| 4 | Summoner | 1, 2 | offered |
| 5 | Reaper | 1, 2 | offered |
| 6 | Crystal Warrior | 1 | offered |
| 7 | Fox Mage | 3 | offered |
| 8 | Archer | 1, 2, 3 | hidden |
| 9 | Spirit Warrior | 3 | offered |

The skeletons are `ROLE_SKEL_MALE` (1), `ROLE_SKEL_FEMALE` (2) and `ROLE_SKEL_LUOLI` (3) (L1 Q4). Which name belongs to which vocation is L1's reading; the English strings are D2's.

## 6. The create result

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 6.1 | **628's handler.** If the actor is a table with a `pid`, the handler appends it to its copy of the list. Then it fires `EVENT_CREATE_ACTOR_RESULT(actor)`. | **Observed** (constants), **Inferred** (flow) | S1 `0.33` |
| 6.2 | **The listener.** It asserts `type(actor) == "table"`, appends the actor to the select screen's list (a new list if there is none), closes the create screen and calls `ShowSelectActor(actor.pid, list)`. So the new character becomes the selection. | **Observed** (constants), **Inferred** (flow) | S9 `0.0`; S8 (`OnBtnCreateActorClick`) |
| 6.3 | **The actor's fields.** The new actor goes through the same list builder and preview as a 228 actor, so it carries the same fields (§4). Without a `pid`, `ShowSelectActor` has no selection and sends 292 instead of drawing. | **Inferred** | S6 `0.16`, `0.3` |
| 6.4 | **Its place.** Fenji gives the new actor `slot` = the number of the account's other characters + 1, the place after them, so the builder needs no 321. | **Inferred** | S6 `0.3`; `lobby::create_result` |
| 6.5 | **The wait.** The create panel disables its `BtnNext` button right after it sends 304, and sets no timer. Only a 270, whose listener re-enables it (§8), or showing the panel again does. So every 304 needs a 628 or a 270. A refusal is a 270 alone: a 628 carrying nil or a non-table fails the listener's assert (caught), and the button stays disabled. | **Observed** (no timer), **Inferred** (effect) | S12 `0.1` [88]–[101], `0.11`, `0.17` (L1 Q6) |

## 7. Delete

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 7.1 | **The typed answer.** The dialog asks for `tDELETE_ANWSER = _L("古剑奇谭")`, which in English is "Swords of Legends Online"; its prompt, `tDELETE_PROPMT`, names it. The confirm handler compares the typed text with it exactly. A mismatch shows an error and sends nothing. The German and French words are **Unknown**: those caches weren't read. | **Observed** (the strings), **Inferred** (the comparison) | S11 `0`; S8 `0.0`; D2 |
| 7.2 | **166** carries the selected pid (`NET.DeleteActor(selectActorPID)`). | **Observed** | S8 `0.0`; S1 `0.51` |
| 7.3 | **84's three values.** The handler fires `EVENT_DELETE_ACTOR_RESULT(pid, ms, level)`. The listener re-enables the delete button and hides the dialog. Then, inside `SYSTEM.TracedCall`, it looks for the actor whose `pid` equals the first value. If `(prop_base_lv or 1) >= level`, it shows the "deletion pending" alert ("角色删除准备中，{1}内再次登录可取消。…", with `ms / 1000` as a time and the level as a rank text). Last, it clears the selection and calls `QueryActors` (370, 496, 292). | **Observed** (constants), **Inferred** (flow) | S1 `0.34`; S9 `0.1`, `0.1.0` |
| 7.4 | **Fenji's values: (pid, 0, 2147483647).** The pid names the deleted character. The time is 0 ms, since nothing is pending (spec D9). 2147483647 (2^31 − 1) is a level no character reaches, so no alert shows and the list refreshes. The third value must be a number: with nil the comparison raises inside `TracedCall`, and whether that native call catches the error, so that the refresh still runs, is **Unknown**. | **Inferred**; a nil level **Unknown** | S9 `0.1.0`; S17 (`TracedCall` is `__script_system_trace_call`); `lobby::delete_result` |
| 7.5 | **After the delete.** The next 228 leaves the character out, and an empty list returns the player to the create screen (3.10). An unknown pid gets 270 `ERR_ROLE_NOT_FOUND` instead. Nothing disables the delete button while 166 is out, so it stays usable after a refusal. | **Inferred** | S5 `0.9`; S9 `0.1`, `0.2`; S6 |

## 8. Errors

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 8.1 | **Message 270.** Its handler takes a code and format arguments. It prints the code's text to chat (channel 63, unless the row names others), except for `ERR_GLOBAL_NULL` (1025238128395601421), which it skips. It fires `ERR.EVENT_SERVER_ERROR(code, …)`. Only for `ERR_ROLE_LEAVE_FORBID` (1270344823122462536) does it also re-enable sending. A string code is printed as itself. | **Observed** (constants), **Inferred** (flow) | S16 `0`, `0.8`, `0.3`, `0.0` |
| 8.2 | **The codes are the `errors` sheet's `ID`s.** The sheet (`content_system_get_table("errors")`) is keyed by `ID`, a 64-bit integer, and `literal` names each row (`ERR_*`). `GetErrorCodeText` indexes the sheet with the code itself, so a code goes out as that integer (pickle tag `06`); a float can't hold these values exactly. | **Observed** (the sheet), **Inferred** (the encoding) | S16 `0.6`; D4 |
| 8.3 | **Who shows them.** The create panel's listener (scope `CreateActorPanel`) puts the text in its error frame and re-enables `BtnNext`. The select screen's listener (scope `ActorSelectFrame`) shows a "Hint" alert for any numeric code with text, except `ERR_SERVER_IS_FULL` (3103948387299755697). It always restores the screen: the frame takes the mouse again, `is_disabled` is cleared and the 10 s `actorentertime` timer is killed. Both listeners stay registered, so a refused name may show the inline text and the alert together (§13). A row whose text holds a `<w … button=…/>` mark gets an alert button that opens a web page of the original operator (S9 `0.2`); the four `ERR_POLICE_LOGIN_ERROR_*` rows have it, and none of Fenji's codes do, which later choices should keep. | **Observed** (constants), **Inferred** (flow; both at once) | S12 `0.11`, `0.15`; S9 `0.2`, `0.3`; D4 |
| 8.4 | **The codes Fenji uses** (`fenji_proto::lobby::code`), in the table below. Each matches the sheet's `ID` (L2). | **Observed** (rows, texts); the choices are Fenji's | D4; D2 |
| 8.5 | **Codes left out.** `ERR_ROLE_VOC_GENDER_NOT_MATCH` (-8225785320432851761): the request carries no gender. `ERR_SERVER_IS_FULL`: the select screen shows nothing for it. `ERR_GLOBAL_NULL`: it isn't printed, but the select screen would still alert "Silent error". The sheet has no code for a vocation that isn't open (L1 Q4). | **Observed** (the rows), **Inferred** (the alerts) | D4; S9 `0.2` |

| Situation | `literal` | `ID` | English text |
|---|---|---|---|
| The name is taken | `ERR_ROLE_NAME_EXIST` | 5391046069930669593 | "This character name already exists" |
| The name's length is wrong | `ERR_ROLE_NAME_LENERR` | -2076552961395212067 | "Invalid character name length" |
| The name has other characters | `ERR_ROLE_NAME_ALLCHINESE` | -2789480703151742396 | "Permissible characters: letters a-z, umlauts, grave/acute accents, cedillas, full stops (except at start/end of name), hyphens, underscores" |
| The request is malformed | `ERR_ROLE_NAME_ILLEGAL` | -2195789287712819046 | "Character name contains forbidden words" |
| The vocation and skeleton don't match | `ERR_ROLE_VOC_SKELETON_NOT_MATCH` | -7591047980372978664 | "The class and body type do not match." |
| Too many characters | `ERR_ROLE_MAX_NUMBER` | 8663116806817837223 | "The number of characters has reached its limit" |
| An unknown pid (166) | `ERR_ROLE_NOT_FOUND` | -2776944664988636544 | "Character not found" |
| Entering the world (10) in M3 | `ERR_SERVER_NOT_RUNNING` | 7248304305738093764 | "The server isn't open yet" |
| The server's own failure | `ERR_SERVER_NOT_RESP` | -5759203401640685215 | "Server not responding" |

## 9. Close reasons

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 9.1 | **The close message.** A type-1 message (`STC_PROTOCOL_CLOSE`) closes the connection, and its body, up to 255 bytes, becomes the close reason. | **Inferred** | G1; [`../transport.md`](../transport.md) 7.4 |
| 9.2 | **Where the reason shows.** When the auth is refused (state 7, `CLOSED_AUTH`), Login's timer calls `UIMain.ShowLoginError` with `NET.FetchNetErrorByErr(reason)`, or a fallback text when that returns nothing. `FetchNetErrorByErr` looks the reason up in a table that `net.luac`'s main chunk builds. In M2, run m2-r3b showed an unknown reason as "Internal server error: …". | **Inferred**; the unknown reason **Observed** (M2) | S5 `0.6`; S1 `0`, `0.62`; [`../transport.md`](../transport.md) M2R3B |
| 9.3 | **The keys.** The table has 15 string keys, and the integer keys 2–11 and 14–21. The keys for M3's cases are in the table below. The other string keys are `error server-not-running` ("The server is currently unavailable, please try again later."), `error server-inner-error` ("Internal server error, please try again later."), `error server-logic-error`, `error auth-client-token-error` (the same text as `error auth-failed`), `error auth-client-dictionary`, `error bad-gameforge-ticket`, `error account-banned-by-gameforge`, `error account-not-own-game`, `error kick-by-gm`, `error kick-by-user`, `error billing-login-no-point`, `error addiction-limit` and `error kick-by-addiction`. | **Observed** | S1 `0` (decompiled lines 1674–1810); D2 |
| 9.4 | **No key for a full server.** For a reason the table lacks, `FetchNetErrorByErr` tries three things. A string `NAME=arg` is looked up as `ERR.GetErrorCodeText(__enums__[NAME], arg)`; `enums.luac` makes each `ERR_*` literal in `data/tables/errors.tab` an enum name, and that file ships ([`../../re/m2-tables.md`](../../re/m2-tables.md) 1.1). A number is looked up as an error code. Anything else shows "Internal server error: {1}" with the reason. So spec D17's plain "server full" shows as "Internal server error: server full". A reason `ERR_SERVER_IS_FULL=1` would probably show the sheet's "The server is full", but only if the enum values equal the sheet's `ID`s. `enums.luac` computes them with the native `ks.pickle_hash`, so that stays **Unknown** until a run (§13). | **Inferred**; the `NAME=arg` route **Unknown** | S1 `0.62`; S20 `0`; D4 |
| 9.5 | **Fenji's choices.** `error auth-failed` for a refused ticket or account (spec D10), `error duplicate-login` for the connection that a second login replaces (spec D17), and `error server-inner-error` when the server's own store fails during the ticket check. A full server gets spec D17's plain text, unless a run shows that the `NAME=arg` route works. | the choices are Fenji's | M3 spec D10, D17; `crates/fenji-server/src/gate.rs` |

| Case | Key | English text |
|---|---|---|
| Auth failure | `error auth-failed` | "Account authentication has failed. Please try again later." |
| The server's own failure | `error server-inner-error` | "Internal server error, please try again later." |
| Duplicate login | `error duplicate-login` | "Your account is offline due to repetitive logins (error code: 403). If this wasn't you, please verify the security of your account and try to log in again later." |
| Full server | none (9.4) | |

## 10. Entering the world

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 10.1 | **Enter.** The enter button returns at once if the screen is disabled, a global alert is open or nothing is selected. It refuses a blocked character (4.3) with "You cannot log into the game as your character is blocked.". Otherwise it disables the screen, arms a 10 s timer (`SetDelayTimer("actorentertime", 10000, …)`) that re-enables it, and calls `UIMain.RequestEnterScene(pid)`, which sends 10 with the pid. | **Observed** (constants), **Inferred** (flow) | S8 `0.1`, `0.1.1`; S23 `0.17`; S1 `0.49` |
| 10.2 | **What success would be.** Nothing on the select screen handles a reply to 10: it waits for the world. The world may first queue the player (605 `PROTOG2C_SCENE_QUEUE`, 728 `PROTOS2C_SCENE_QUEUE_FINISH`) and then loads the scene with 772 `PROTOS2C_ACTOR_LOAD_SCENE`. That is M4's. | **Inferred** | S22; D1; L1 Q10 |
| 10.3 | **M3's refusal: 270 `ERR_SERVER_NOT_RUNNING`** (7248304305738093764, "The server isn't open yet"). The select screen shows it in a "Hint" alert and restores itself at once (8.3). Without it, the 10 s timer would restore the screen anyway. The screen's own unused table, `tErrorsMapping`, lists the refusals it was written for, keyed by `ID`: this code first, then scene-login codes. `ERR_SERVER_IS_FULL` would show nothing, and `ERR_GLOBAL_NULL` would alert "Silent error". | **Observed** (text, table), **Inferred** (the choice) | S11 `0`; S9 `0.2`; D4 |

## 11. What Fenji sends

M3's server answers as follows (`crates/fenji-proto/src/lobby.rs` and `packet.rs`). Integers take the shortest pickle tag that holds them, so the error codes and large pids go as `i64`, tag `06` ([`../payloads.md`](../payloads.md)). Every script message goes through `packet::script_body`, which refuses the ids of 2.3. 228 and 628 are never sent unasked (2.4).

| The client sends | Fenji answers | Minimal values |
|---|---|---|
| its auth message, accepted | `ok`, then 414 | `(414, nil, {times_remain = 0, coins_remain = 0, bind_coins_remain = 0, cards_endtime = 0}, nil)` (`packet::set_net_connected`; [`../transport.md`](../transport.md) 10.4) |
| its auth message, refused; or a second login to the account | a close (§9) | `error auth-failed`; `error server-inner-error` when the server's store fails; `error duplicate-login` to the replaced connection |
| 402 | nothing | 495 is optional. Without it, on the launcher path, the select screen named the server "Fenji", the server name in `gj_uinfo` (m3-l3b; that `NET.Login` stores it from there is **Inferred**). L1 Q1 read the label's default, "Swords of Legends Online" |
| 370, 496 | nothing | |
| 292 | 228 | `(228, last_pid or 0, {actor, …}, {rename_num = 0, reborn_num = 0, change_gender_num = 0, role_num = max_characters − 9, at least 0})` (`lobby::role_list`): the actors in slot order with `slot` 1, 2, …, and `{}` when there are none |
| (each actor) | | `pid`, `slot`, `prop_base_name`, `prop_base_lv` (1), `prop_base_vocation`, `prop_base_gender` (0 for skeleton 1, else 1), `prop_base_skeleton`, `prop_sys_portrait`, `prop_sys_avatar_feature`, `prop_sys_equips = {}`, `prop_sys_avatar_feature_face` when stored, and the eleven show flags of §4 as `true` (`Actor::to_table`). Never `enable`, `block_cd` or `delete_cd` |
| 304, accepted | 628 | `(628, actor)`, with `slot` = the number of the account's other characters + 1 (`lobby::create_result`) |
| 304, refused | 270 | `(270, code)`, with a code from 8.4 (`lobby::error_code`) |
| 166, a known pid | 84 | `(84, pid, 0, 2147483647)` (`lobby::delete_result`); the client then sends 370, 496 and 292 |
| 166, an unknown pid | 270 | `(270, ERR_ROLE_NOT_FOUND)` |
| 10 | 270 | `(270, ERR_SERVER_NOT_RUNNING)` |
| 321, 26, 454 | nothing | 509 only logs; renaming and rebirth aren't in M3 |
| 392 | closes the connection | the client quits 5 s later whatever the server does (L1 Q12) |

## 12. The platform paths

Which login path a player's client takes decides what reaches the gate, so it was read for gate #114. The reading is static: REA on `bin64/gameforge_api64.dll` (two sessions) and on the exe (one session), every session closed, plus the scripts. Nothing was run, loaded or patched for it. The full notes are L4, and the keys P1–P3, P5, X1 and X3–X5 below resolve there and in [REA Evidence IDs](#rea-evidence-ids). Statements about Valve's own code come from Valve's public documentation as the reader remembered it, not re-checked, and are marked *external*. **The owner decided [gate #114](https://github.com/theintonerzero/fenji/issues/114) on 2026-10-13: option (a).** The launcher verifies the client files against the pinned manifest, then starts the unmodified client on the launcher path (12.6) with `gj_uinfo`, `gj_gates` and a single-use Fenji ticket; no platform credentials are involved, and ownership rests on the host's vetting (ground rule 1). Still open: whether the AGT driver loads on native Windows on this path, to be read statically first (ground rule 6).

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 12.1 | **The client's platform type is always 3, Gameforge, in this build.** `platform_get_type` (FUN_1401E89D0) pushes DAT_1431F3620, which only FUN_1401E2450 writes, with the constant 3. FUN_14024EA30 calls it on both start-up branches, unconditionally. | **Inferred** | X4 |
| 12.2 | **So the scripts' own Steam branch never runs.** `NET.Login`'s `platform type == 4` case (server id 99, zone `"80"`, account type `"steam"`) can't be reached, and the Steam bindings it calls are stubs: `steam_get_actor_id` and `steam_get_actor_ticket` push nil (FUN_1401E9410), `steam_get_ticket_state` pushes true. Its fallback gate fetch (`upgrade-api.wangyuan.com/game/gate?gid=105`, then `121.5.97.203:8001`) is reachable only from that branch. | **Inferred**; constants **Observed** | X3, X4; S1 `0.60`, `0.57` |
| 12.3 | **With `--gf-sdk=steam` the client logs in through the Gameforge branch.** The platform library delay-loads the client's own `steam_api64.dll` (Valve's, FileVersion 06.28.18.86) and calls `SteamAPI_Init`. The exe goes on only when `SteamAPI_IsSteamRunning`, `SteamAPI_Init` and `ISteamUser::BLoggedOn` all succeed, so a Steam client must be running and signed in (spike-0's run 3 ended for want of it). The library makes no ownership call of its own: no `BIsSubscribed*`, no `UserHasLicenseForApp`, no encrypted ticket. Any licence check is inside Valve's `SteamAPI_Init` (*external*). | **Inferred**; the run **Observed** | P1, P3, X1; R3 |
| 12.4 | **What reaches the gate on the Steam path.** The ticket is the hex of a JSON object built in the library (FUN_1800A2790): `auth_type` `"steam"`, the Steam session ticket (itself hex), the SteamID64, the persona name, the app id and, with `--gf-sgs-enabled`, an Iovation fingerprint. The account field is the persona name, and a new session ticket is fetched at every login. Fenji can read the SteamID64 but can't verify any of it without Valve: the Web API needs the publisher's key, and validating through Valve's game-server SDK is untested. No value of Fenji's rides in that token, and the session ticket must never be stored (#102). | **Inferred**; Valve's routes *external* | P3, X5 |
| 12.5 | **The Gameforge path** loads `gameforge_client_api64_v2.dll` from the folder that a Gameforge Client `Uninstall` registry entry names, resolves four exports (`GameforgeApi_Init`, `_Shutdown`, `_Load`, `_IsContextAvailable`) and asks it for five C++ module objects. A stand-in would be native code running inside `gujianol.exe`. The library's developer settings, among them a switch that turns off a default-on `useSignatureVerification`, are behind a password checked by its SHA-1; it was not, and won't be, recovered (ground rule 6). | **Inferred**; strings **Observed** | P2, P5 |
| 12.6 | **The launcher path, which players use (gate #114, option a).** FUN_14024F010 checks only the environment variable `gj_uinfo`; when it is set, every SDK call is skipped (`LoadSdk`, `IsRunning` and `Init` have no other direct caller; calls through pointers aren't covered), and the exe holds no `gf-sdk` string. The platform DLL still loads, and its `DllMain` runs its configuration search (environment, registry, a file; no network). The client then logs in to `gj_gates` with `gj_ticket` ([`../login.md`](../login.md) 2.8–2.10). No platform service is contacted: in runs en-1 and m3-l6 the client made no name lookup and one connection, to the stub or the gate. The only lookup in their Wine logs is Wine's own, wineboot's, of the sandbox's host name ([spike-0](../../re/spike-0.md) 8.23). It still tries the server list's local fallback file, `D:\wy_debug.json`, and logs that the Gameforge server list failed (m3-l6, §14). Run m3-l6 also showed that the client starts with no `--gf-sdk` flag at all. | **Inferred**; en-1 and m3-l6 **Observed** | X1, P2; E1; M3L6 |
| 12.7 | **The ticket is never hex-decoded.** `net_manager_login` (`net_manager.luac` `0.20`, pc 28–55) has a step that hex-decodes a ticket matching `^%x+$`, but its condition, `type ~= 3 and (3 == 1 or 3 == 4 or 3 == 5)`, compares constants, so the step never runs for any platform type (L4 2.4). Fenji's tickets start with `fenji1-`, which couldn't match anyway. | **Observed** (the constants), **Inferred** (never runs) | S24; L4 |
| 12.8 | **Corrections this reading makes:** [`../login.md`](../login.md) §1's `gj_gates` row described the dead platform-4 branch as "the Steam path" (12.2); spike-0 8.2's configuration search runs in `DllMain`, with `gj_uinfo` set too; spike-0 4.34–4.35's address is `121.5.97.203:8001`, the Chinese gate fallback of 12.2, not a patch download. Each now carries an M3 note. | | L4 |


## 13. Open questions for the runs

Only a sandboxed run of the real client can settle these. The M3 runs (§14) settled 7, 9, 11 and 15, and parts of 1 and 5; the others stay open.
1. **Settled for Fenji's values (§14):** with `prop_sys_equips = {}`, a face string and every show flag `true`, the select screen built and showed the character with the creation screen's default face, dressed but not in the outfit shown at creation; that the dress is the vocation's default is **Inferred**. Whether the stored face string was applied, or a fallback looks the same, is **Unknown**: the run kept the default face. The look with no face string, or with the show flags absent, is still **Unknown**. The question was how the select screen's model looks with `prop_sys_equips = {}`, with no face string, and with the show flags absent or `true`: a plain body, or a failure?
2. Whether `SetSelected(true)` in the list builder fires `EVENT_SELECT_STATE_CHANGED` at once. The builder assumes it, since `UpdateReButton` then reads `selectActorInfo` (L1 Q3).
3. Whether a pid above 2^32 or 2^53 survives `lua_actor_new` and the native actor map.
4. Whether `maxBytes="24"` counts bytes or characters.
5. **Partly settled (§14):** the order holds, the three within 0.3 ms, and a click on the title counts as a key. Still open: why one Return on the title sent nothing (m3-l3a), and the logo-video gate, which no run met (every input came after the title was up). The question was the 370, 496, 292 sequence and its timing after a key press, which no M2 run sent, and the logo-video gate.
6. That 270 `ERR_SERVER_NOT_RUNNING` after 10 shows the "Hint" alert and restores the screen, and whether a refused name shows both the inline text and the alert.
7. **Settled (§14):** no alert, the refresh came 0.64 s later, and the empty list opened creation. The question was that 84 `(pid, 0, 2147483647)` shows no alert and that the refresh (370, 496, 292, then 228) shows the new list; and that an empty list returns to the create screen.
8. That an actor without `slot` makes the client send 321, and with how many values: two from the builder, three from a swap.
9. **Settled (§14):** 758 bytes, sent LZ4-compressed as 702, with a 490-byte face string in six sections and a trailing `;` (5.9). The question was the size and compression of a real 304, whose face string comes from native code.
10. The delete word in the German and French clients.
11. **Settled (§14):** nothing visibly broke, and the select screen named the server "Fenji", which only `gj_uinfo` carried (§11's 402 row). The question was that nothing breaks when 495 is never sent. Other add-ons also listen to `EVENT_NET_CONNECTED` (L1 Q1); the server-name label and the window title then keep their defaults.
12. Whether the close reason `ERR_SERVER_IS_FULL=1` shows "The server is full" (9.4).
13. What the rename and reborn buttons do when the account's counters are 0, and whether a client can send 26 or 454 in M3.
14. Whether the native `__script_system_trace_call` catches errors (7.4). Fenji's values don't depend on it.
15. **Settled (§14):** it does (run m3-l6). The question was whether the client starts on the launcher path with no `--gf-sdk` flag at all (12.6).

## 14. Runs

The M3 rungs ran the pinned client in `scripts/spike/run-client.sh`, with its `--server`, `--launch`, `--key` and `--type` options (#136), against `fenji-server`, with the AGT driver watchdog armed (2026-10-13; predictions and outcomes in M3R). Each run:
- started a new `fenji-server` on one database, which held the account `tester`;
- ran `fenji-launch play --print` inside the sandbox once the server was up;
- gave the client only the launcher path's three pairs, with a fresh ticket.

The client got `--gf-sdk=mocked --fs=data:_index:_index` (gate #11); m3-l6 dropped `--gf-sdk=mocked` and kept the rest. Key presses, typing and clicks went through XTEST on the nested display.

The server logged what it received, not what it sent. The replies in the table (228, 628, 84) are what its code answers, which its tests pin byte for byte (`crates/fenji-server/src/lobby.rs`, `crates/fenji-server/tests/lobby.rs`), and the client behaved as those replies make it behave. The client's own log ends at "net connected" in every run, so "no error" below means none on screen.

| Run | What the run did | What happened | Watchdog | Evidence |
|---|---|---|---|---|
| m3-l1 | the handoff only, 180 s | "in the lobby" for `tester` 8.4 s after the server's start; 402 2.6 s later; the English title, "Press Any Key", by 120 s | no sign | M3L1 |
| m3-l2 | a click at 125 s, then Return at 130 s | the click already counted as "any key": 370, 496, 292, then the same three again at the key; each 292 was answered with an empty 228; character creation by 150 s | no sign | M3L2 |
| m3-l3a | Return at 130 s, then clicks at Next's position at 165, 195 and 225 s, to map the creation screens | the Return sent nothing: "Press Any Key" was still up at 150 s (cause **Unknown**); the click at 165 s sent 370, 496, 292 and the empty list opened creation; then the vocation screen, the appearance screen with its name box, and the client's own "Please give the character a name!" at Complete; no 304 | no sign; the watchdog first saw the client as `wine64` and logged "client pid None", so the watchdog's rule S2 (`scripts/spike/watchdog.py`) never armed (#142); both `winedevice.exe` had started before the client | M3L3 |
| m3-l3b | Return; Next; a click on the name box; `Lin` typed; Complete | one 304: name `Lin`, vocation 7, skeleton 3, a 490-byte face string; stored, answered with 628; the select screen showed "Lin", "Mortal 1", with "Current server: Fenji" | no sign; "client pid None" as in m3-l3a, so the watchdog's rule S2 never armed; both `winedevice.exe` had started before the client | M3L3 |
| m3-l4 | a new server process on the same database; Return; a click on the delete icon | 228 listed Lin; the select screen showed Lin by 180 s; the delete dialog opened | no sign | M3L4 |
| m3-l5 | as m3-l4, then `Swords of Legends Online` typed into the dialog and Delete | 166 with pid 1; deleted, answered with 84; 0.64 s later 370, 496, 292; the empty 228 opened creation; no pending-deletion alert | no sign | M3L5 |
| m3-l6 | the handoff with no `--gf-sdk` flag | as m3-l1: logged in, 402, the title | no sign | M3L6 |

**Observed:**
- **The lobby works end to end with the real client.** It covers 228's list, 628's actor and 84's values, and the client acted on each as §3–§7 read it.
- **The creation screen's defaults.** The default pick, Fox Mage and "Girl", is vocation 7 and skeleton 3. The real 304 carried the ten keys of 5.1: voice 3, base face 1, outfit 2 and portrait 169 (`169 << 16`). It was 758 bytes, sent LZ4-compressed as 702.
- **The launcher path needs no SDK flag**, in the sandbox (m3-l6) and on bare metal on Linux (bm-2 to bm-6, [`../../re/m3-summary.md`](../../re/m3-summary.md)). Native Windows runs are untested.
- **The network.** In m3-l6 the client made no name lookup and one connection, to the gate on 127.0.0.1:7800, and tried no other address. The only lookup in the Wine log is wineboot's, of the sandbox's host name, as in spike-0 8.23 and en-1. It did try one URL, the server list's fallback `file:///D://wy_debug.json` ([`../login.md`](../login.md) §1): `wine.log` shows `D:\wy_debug.json` not found, and the client logged "FAILED TO FETCH GAMEFORGE SERVER LIST!". So the list fetch runs on the launcher path even with no SDK flag; it reads a local file and uses no network.
- **No evidence file holds a ticket or the password.** That was checked with `grep` on each run's evidence.

**Inferred:**
- **The server's choices hold.** No `enable`, no `block_cd`, 84's threshold, and empty `prop_sys_equips`: the select screen built and showed the character with no error on screen.

**Unknown:**
- The rest of §13: the look with no face string or absent show flags (1), `SetSelected`'s event (2), pids past 2^32 (3), what `maxBytes` counts (4), the ignored Return and the logo-video gate (5), 270 after 10 and a refused name's alerts (6), 321 (8), the German and French delete words (10), a full server's reason (12), the rename and reborn buttons (13), and the trace call's error handling (14).

## Evidence index

`re/` is git-ignored, so the script listings, caches and notes exist only on the owner's machines. `luatool` regenerates the listings from the pinned exe, and `archive` extracts the caches. A script key names the disassembly `re/m1/disasm/data/<path>.luac.dis` and the decompiled `re/m1/decompiled-all/data/<path>.lua`, with the function paths cited. `AS/` stands for `data/locale/common/interface/addons/ActorSelect/`, `AC/` for `data/locale/common/interface/addons/ActorCreate/`, and `PG/` for `data/locale/common/core/modules/pregame/`.

| Key | Path or address | What it holds |
|---|---|---|
| S1 | `data/core/modules/net/net.luac` | The main chunk `0`: the handler registrations ([118]–[155]) and the close-reason table (decompiled lines 1674–1810). The handlers for 414 (`0.29`), 495 (`0.30`), 786 (`0.31`), 228 (`0.32`), 628 (`0.33`), 84 (`0.34`), 729 (`0.38`), 368 (`0.42`), 509 (`0.43`) and 598 (`0.46`). The senders `QueryActors` (`0.48`), `SelectActor` (`0.49`), `CreateActor` (`0.50`), `DeleteActor` (`0.51`), `RenameActor` (`0.52`), `RebornActor` (`0.53`) and `ChangeActorSlot` (`0.54`). `FetchNetErrorByErr` (`0.62`) |
| S2 | `data/core/client/hotpatch_system.luac` | The 243 handler (`0.2`), the 216 handler (`0.3`), `hotpatch_pull` (`0.4`), `hotpatch_fetch_content` (`0.5`) |
| S3 | `data/core/client/net_manager.luac` | The script dispatch (`0.15`) and the 556 runner (`0.14`) |
| S4 | `data/core/client/event_system.luac`; `data/core/client/module_interface.luac` | `event_set_fire` (`0.8`) and `event_system_fire_event`; `RegisterEvent` |
| S5 | `data/interface/addons/Login/Login.luac` | The queue display (`0.4`), the `login` timer (`0.6`), the next step (`0.9`), the 228 listener (`0.10`), `OnGlobalKeyUp` (`0.24`) |
| S6 | `AS/ActorSelect.luac` | `UpdataActorModel` (`0.0`), the list builder (`0.3`), `changeActorSlot` (`0.5`), the 228 listener (`0.10`), `OnLoad` (`0.12`), `ShowSelectActor` (`0.16`), `HideSelectActor` (`0.17`); the main chunk `0` |
| S7 | `AS/ActorFrame.luac` | `updataCD` (`0.1.6`), `SetData` (`0.1.7`) |
| S8 | `AS/ActorSelect_Control.luac` | The delete confirmation (`0.0`), the enter button (`0.1`, with its timer `0.1.1`) |
| S9 | `AS/ActorSelect_Updata.luac` | The 628 listener (`0.0`), the 84 listener (`0.1`, `0.1.0`), the 270 listener (`0.2`), their registrations (`0.3`) |
| S10 | `AS/ActorSelect_ShoppingMall.luac` | `SetAccountReNum` (`0.0`), `UpdateReButton` (`0.4`) |
| S11 | `AS/Config.luac` | The main chunk `0`: `tErrorsMapping`, `tDELETE_ANWSER`, `tDELETE_PROPMT` |
| S12 | `AC/CreatePanel.luac` | The create request (`0.1`), the 270 listener (`0.11`), its registration (`0.15`), `ShowCreatePanel` (`0.17`) |
| S13 | `AC/ActorCreate.luac`, `AC/VocationSetting.luac`, `AC/EquipPanel.luac` | `SetActorSkelVocation` (`ActorCreate.luac` `0.0`); the skeleton buttons (`VocationSetting.luac` `0.3`) and `InitVocationSetting` (`0.12`); the outfit buttons |
| S14 | `PG/pregame_actor.luac`, `PG/pregame.luac`, `PG/face_control.luac`, `PG/pregame_camera.luac` | `create` (`pregame_actor.luac` `0.0`) and `change_actor` (`0.4`); `ChangeActor` and `GetRoleSkeleton`; the face loader (`face_control.luac` `0.2`) and `GetFaceData` (`0.11`) |
| S15 | `data/core/share/face_verification.luac` | The vocation and skeleton tables (the main chunk `0`), the face validator (`0.35`), `check_base_face` (`0.36`), the default face (`0.37`), `get_role_skel` (`0.43`) |
| S16 | `data/core/modules/err/err.luac` | The main chunk `0` (`SetStub(270, …)`, `SetStub(720, …)`), the row lookup (`0.0`), the printer (`0.3`), `GetErrorCodeText` (`0.6`), the 270 handler (`0.8`), the 720 handler (`0.9`) |
| S17 | `data/core/client/module_system.luac`; `data/core/share/locale_config.luac`; `data/core/modules/system/system.luac` | `load_module_specs` (`0.20`) and `load_localed_module_spec` (`0.22`); the locale choice (`0`); `RequestQuitGame` (`0.5`), `GetServerTimeMS` (`0.17`), `LegalNameInput` (`0.62`), `GetServerLocale` (`0.98`), `IsCommonLocale` (`0.100`) and `TracedCall` |
| S18 | `data/core/client/time_manager.luac` | `server_time_now`, `time_manager_update`, `time_manager_time_sync_resp` |
| S19 | `data/core/client/actor_manager.luac` | `actor_system_create_actor` and its existence check |
| S20 | `data/core/share/enums.luac` | The main chunk `0`: the protocol names from the protocol table, and the `ERR_*` names from `errors.tab` |
| S21 | `data/core/client/content_system.luac` | The 627 handler (`0.13`) |
| S22 | `data/core/modules/queue/queue.luac`; `data/core/client/scene_manager.luac` | The 605 and 728 handlers (`0.27`, `0.28`); the 772 handler (`0.56`) |
| S23 | `data/interface/system/UIMain/UIMain.luac` | `RequestEnterScene` (`0.17`) |
| S24 | `data/core/client/net_manager.luac` | `net_manager_login` (`0.20`), pc 28–55: the hex-decode step, whose condition compares constants |
| D1 | `data/caches/protocol_ver.new_cache` (copy: `re/m2/tables/protocol_ver.new_cache`) | The client's protocol table: id to name in both directions, and `ver` |
| D2 | `data/caches/locale_en.new_cache` (copy in `re/m2/tables/`) | The English strings |
| D3 | `data/caches/all_tables.new_cache` (copy in `re/m2/tables/`) | `vocation_skel_templ`, `open_vocation`, `vocation_initequip_templ` |
| D4 | `re/m3/tables/errors.jsonl` (`archive dump --table errors`) | The `errors` sheet: `ID`, `literal`, source text |
| D5 | Under `re/m1/extracted/data/`: `locale/gameforge/load.config`; `locale/common/interface/addons/ActorCreate/FeatureSetting.xml` (line 517) and `VocationSetting.xml` (line 226); `locale/common/interface/addons/ActorSelect/ActorFrame.xml` (lines 139, 175); `interface/addons/Login/Login.xml` (line 71) | The locale's default, the name box, the vocation buttons, the select screen's entries, the title prompt |
| G1 | FUN_14029A870 | The type-1 close: the body becomes the close reason ([`../transport.md`](../transport.md) T15, 7.4) |
| G2 | FUN_140222E00, FUN_1400D5850 | Inbound messages by type: a type-0 body is unpickled before the scripts' handler runs ([`../opcodes.md`](../opcodes.md) O.1) |
| L1 | `re/m3/lobby-reading.md` | The reading agent's notes (Q1–Q12), with line ranges in the decompiled files |
| L2 | `re/m3/lobby-check.md` | The checks of L1's five load-bearing claims, and of 2.1, 2.3 and §9 |
| L3 | `re/m3/planning-findings.md` | The M3 plan's first probes, which L1 corrects (L1 §7) |
| M3R | `re/m3/runs.md` | The M3 rungs' predictions, written before each run, and their outcomes |
| M3L1–M3L6 | `re/runs/out/m3-l1/`, `m3-l2/`, `m3-l3a/` and `m3-l3b/` (M3L3), `m3-l4/`, `m3-l5/`, `m3-l6/` | Each run's `inputs.txt`, `server.log` (trace), `io/launch.txt` (the ticket redacted), `wine.log`, screenshots and watchdog log |
| L4 | `re/m3/platform-reading.md`, with its REA bundles in `re/m3/rea/` | The platform reading behind §12: every P and X key with its exact REA call and Evidence ID |
| P1–P5 | `bin64/gameforge_api64.dll` (SHA-256 `1250487ab0639ff5e16bc2042a9fdb8ffdfe1c3e1cfef77eacc2695c60d5908f`) | P1 SDK selection (FUN_180008040, FUN_180011890); P2 `DllMain` and the configuration reader (FUN_1800018A0, FUN_1800137D0, FUN_1800142D0); P3 the Steam module (FUN_1800A1740, FUN_1800A2790); P5 the Gameforge module's loader (FUN_1800975B0, FUN_180097880) |
| X1–X5 | `bin64/gujianol.exe` | X1 the `gj_uinfo` check (FUN_14024F010) and the SDK start-up (FUN_1401A34A0); X3 the platform bindings (FUN_1401E8710, FUN_1401E9410); X4 the platform type (FUN_1401E89D0, FUN_1401E2450, FUN_14024EA30); X5 the Gameforge getters (FUN_1401E7960) |
| R3, E1 | `re/spike/out/r3-steam-sdk-eq/wine.log`; `re/runs/out/en-1/wine.log` | Run 3: `SteamAPI_Init` fails without a Steam client. Run en-1: the configuration search, one connection |

### REA Evidence IDs

§1–§11 opened no REA session: the native functions they cite were read through REA for [`../opcodes.md`](../opcodes.md), and those IDs are copied from there. §12's come from the platform reading's three sessions (L4), which lists every call; the table gives the ones its findings rest on. An ID covers its call exactly as sent ([`../../re/README.md`](../../re/README.md#how-to-cite)).

| Function | Cited in | Call | Evidence ID |
|---|---|---|---|
| FUN_1400D5850 | G2 | `procedure_pseudo_code {procedure: "0x1400d5850"}` | `ev_af1c4fbf5d6aed889b7d529dd45f721617274b50f2625f323d3038aae2e57c64` |
| FUN_140222E00 | G2 | `procedure_pseudo_code {procedure: "0x140222e00"}` | `ev_4656519e08dab96fa5ad38e228a578375ef1b30907fb075530e4d9e15b4d3d20` |
| FUN_14029A870 | G1 | `procedure_pseudo_code {procedure: "0x14029a870"}` | `ev_4287eb1c9108d48b68771d0a270f2344b05732bb6c87fa64aff4e688271065a1` |
| FUN_14024F010 | X1, 12.6 | `procedure_pseudo_code {procedure: "0x14024f010"}` | `ev_a3fcb94db8ff9db03a63277fd36998063ab52d2143c39e1bbae5184f4dfe39a4` |
| FUN_1401A34A0 | X1, 12.3 | `procedure_pseudo_code {procedure: "0x1401a34a0"}` | `ev_72eda26529f7632d326bcff40f70112ba4a7a3105c295e8cd10f8031c51eb12e` |
| the exe's strings | X1, 12.6 | `search_strings {pattern: "gf-sdk"}` | `ev_6342aa1ca911bb888cd3ad3d85ccefe73426148f71eea4b010f85e5ff20061b4` |
| FUN_1401E2450 | X4, 12.1 | `procedure_pseudo_code {procedure: "0x1401e2450"}` | `ev_a0bb45d049356fef32089da34fb075dc2e88decec32f7f9d683882771d69c87f` |
| FUN_14024EA30 | X4, 12.1 | `procedure_pseudo_code {procedure: "0x14024ea30"}` | `ev_65c90302940d38f6bfe3b0b479fe1b015b53e8ed624db484c8ba5628603f9c03` |
| FUN_1401E9410 | X3, 12.2 | `procedure_pseudo_code {procedure: "0x1401e9410"}` | `ev_82eb62c53ea24afea8458d5b511c7aa309a819383e384784a884b4894b68c561` |
| FUN_1800A1740 | P3, 12.3 | `procedure_pseudo_code {procedure: "0x1800a1740"}` | `ev_f201fc51279250e06451eff35bb7d816f5e13fc7250880a7abdacded1f8caff9` |
| FUN_1800A2790 | P3, 12.4 | `procedure_pseudo_code {procedure: "0x1800a2790"}` | `ev_9396bdd0a91df377433680c9971dfa84ae081da8e8c56b37c5e8fe4a60cef93d` |
| FUN_1800975B0 | P5, 12.5 | `procedure_pseudo_code {procedure: "0x1800975b0"}` | `ev_534e4dd20007f96fef452b060bca835fe2fa84de04ebd5b1a966ca1aa96ae590` |
| FUN_180097880 | P5, 12.5 | `procedure_pseudo_code {procedure: "0x180097880"}` | `ev_1f8dca131d1afbcefcd940610c3289d99c06bc42a694b78329c6363bd01e6cf5` |
| FUN_1800142D0 | P2, 12.5 | `procedure_pseudo_code {procedure: "0x1800142d0"}` | `ev_e1f631d2d7c9fd4c5f008524ffe20da4e7cbd407acfcbe4d77e22ee301e9520d` |
| FUN_1800018A0 | P2, 12.6 | `procedure_pseudo_code {procedure: "0x1800018a0"}` | `ev_65ea895f2cddacaf485a3ca35cccc1997431df5ff58d3f70910329837ed14356` |
| FUN_1401E89D0 | X4, 12.1 | `procedure_pseudo_code {procedure: "0x1401e89d0"}` | `ev_d7d4e0cc9686c610a859ecd8e12a8de71562f836f429412f0f4a82c5c828d454` |
| DAT_1431F3620 | X4, 12.1 | `xrefs {address: "0x1431f3620"}` | `ev_e14737acbfb543ebbc87311cb2531a28f76b21a5a7cb0fdf6b49288a8e5fbb20` |
| FUN_1401E2450 | X4, 12.1 | `procedure_callers {procedure: "0x1401e2450"}` | `ev_eb7e08e78551dd135ba861819542f70ae333c900a3c9a384691a5dc0ecfafd39` |
| `GameforgeApi_LoadSdk` (IAT) | X1, 12.6 | `xrefs {address: "0x14115ac10"}` | `ev_40628875a19cd9757c654686e260a31ca57b13bbfa035e88e0b3f8447674b382` |
| `GameforgeApi_IsRunning` (IAT) | X1, 12.6 | `xrefs {address: "0x14115ac20"}` | `ev_98fa86ff816f549462582647663c386a8bc470ebae4ada2449e5baf31295bd94` |
| `GameforgeApi_Init` (IAT) | X1, 12.6 | `xrefs {address: "0x14115ac30"}` | `ev_86a9991e167a19c65ab638ac0a5465a6789d2476b1becea94a933fe1b4000b25` |
| FUN_1401A3580 | X1, 12.6 | `procedure_callers {procedure: "0x1401a3580"}` | `ev_17480d4e198fd94ce748262b541d8c3717f45dce153233104c0c0454fbf4a682` |
| FUN_1401A34A0 | X1, 12.6 | `procedure_callers {procedure: "0x1401a34a0"}` | `ev_e944b0d2e0fe5d1037c32a309bbe03136a30a50a113936fafdc255ed4b7231b8` |
| FUN_1401E8710 | X3, 12.2 | `procedure_pseudo_code {procedure: "0x1401e8710"}` | `ev_b8833eb2aa9fb91ea60cecff0721a55e0383259d1639a3e5b336421730588994` |
| FUN_1402482F0 | X3, 12.2 | `procedure_pseudo_code {procedure: "0x1402482f0"}` | `ev_70c8c70da9c85db42b6ba63bc4f4218a92fefe77510faa61c147ba9b796fa137` |
| FUN_180008040 | P1, 12.3 | `procedure_pseudo_code {procedure: "0x180008040"}` | `ev_e6dd29639ab9d4f4f47a08dfa774679ddda47ff7ed1080078451f44c44e1c564` |
| FUN_180011890 | P1, 12.3 | `procedure_pseudo_code {procedure: "0x180011890"}` | `ev_1ac0196b6b888b10a9ec152f99629a9b21baefeefa6e10acf641946e38c8ba24` |
| FUN_1401E7960 | X5, 12.4 | `procedure_pseudo_code {procedure: "0x1401e7960"}` | `ev_91e6d3a7b15e0b5778e45d6762065adc7c8690b0a6cdf4c5e9910b3846c5682a` |
| FUN_1401A3690 | X5, 12.4 | `procedure_pseudo_code {procedure: "0x1401a3690"}` | `ev_ea251e06558df384bc72f54c53192381f1bcf38837c3181759696dacb7a7c91b` |
