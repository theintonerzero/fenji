# World: entering

After the select screen's 10, the server answers with the script message 772. The client reloads its game modules, sets the player's actor id and loads the scene's map behind its loading screen, sending a time sync once a second from then on. When the map and the game's add-ons have loaded it reports the native type 3. Only then does the server send the player's own actor (type 7, with a property blob, which carries its move speed) and type 8, after which the loading screen closes and the client's in-world script messages flow. This document gives the evidence behind the values that `fenji_proto::world` uses for that, the format of the property blob that types 7 and 10 carry, the time syncs, and the creation outfit.

**Method.** This reading comes from the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`). It used three sources, all read locally and read-only:
- the client's own scripts, disassembled and decompiled by `luatool` ([`../../re/m1-script-format.md`](../../re/m1-script-format.md));
- the client's own caches: the protocol table, `prop_tables`, two sheets of `all_tables` and `locale_en`, read with a standard-library probe of [`../payloads.md`](../payloads.md)'s format;
- the exe, in the shared Ghidra project and then through REA in one session (2026-10-14), after the evidence norm ([`../../re/README.md`](../../re/README.md#the-norm)); the Evidence IDs are in [REA Evidence IDs](#rea-evidence-ids). Where Ghidra's analysis made no function (0x14020DE10, 0x140210A30), the bytes were read through REA and disassembled with `objdump`. The exe's export table and its built-in property table were read with a standard-library PE reader.

A design-time survey (L4) gave the leads, and each finding here was checked against the bytecode or the exe. Nothing was run: no client, no debugger, no script in Ghidra, and nothing in the shared project was renamed or changed. Written by a session that has not read any source rejected in [`../../re/sources.md`](../../re/sources.md). Tags follow [`../../re/spike-0.md`](../../re/spike-0.md): **Observed** (literally in a file: a constant, a bytecode operand, a table value, bytes of the exe), **Inferred** (our reading of the code), **Unknown**. Script evidence cites the engine path and the function path in its disassembly (such as `0.56`), with bytecode positions in brackets; where the decompiled text and the bytecode differ, the bytecode wins. Native evidence cites functions and addresses in the pinned exe. Evidence keys resolve in the [Evidence index](#evidence-index).

## 1. The sequence

| Step | Direction | Message | What happens | Evidence |
|---|---|---|---|---|
| 1 | C→S | 10 `PROTOC2S_ROLE_SELECT` (0xFF), the pid | the select screen disables itself for up to 10 s and waits ([`lobby.md`](lobby.md) 10.1) | lobby.md §10 |
| 2 | S→C | 772 `PROTOS2C_ACTOR_LOAD_SCENE` (type 0) | reloads the game modules on a first entry, sets the local actor id, caches type-0 sends, starts the load (§2) | S1 `0.56` |
| 3 | C→S, S→C | type 2 time sync, and its reply | from 772 on, once a second (§7) | S4 `0.1` |
| 4 | C→S | type 3 `CTS_PROTOCOL_CLIENT_ENTER`, 21 bytes | sent when the map and the game's add-ons have loaded; the loading screen stays up (§3) | S6 `0.9`; S7 `0.61` |
| 5 | S→C | type 7 `STC_PROTOCOL_SYNC_ENTER` for the pid | creates the player's actor; 8 frames later the loading screen closes (§4, §5) | S2 `0.9`; S3 `0.33`; S6 `0.30` |
| 6 | S→C | type 23 `STC_PROTOCOL_SYNC_PROP_FORCE`, property 1325 | sets the move speed (§6); Fenji doesn't send it, since the blob's 1325 suffices (6.5) | X8 |
| 7 | S→C | type 8 `STC_PROTOCOL_SYNC_ENTER_FINISH` | turns the client's type-0 sends back on and flushes what it cached (§6) | S2 `0.10`; S3 `0.45` |
| 8 | C→S | the cached script messages, as type 0 | logs and fetches; none needs an answer to stand and walk | S2 `0.31` |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 1.1 | **The player's own type 7 must come after type 3.** `UpdateLoadScene` calls `SCENE.NotifySceneLoaded()`, which sends type 3, and in the same step sets `STATE_WAIT_ACTOR_ENTER` (10). `OnLocalActorEnter` moves on to `STATE_PRE_GAME_PLAY` only in that state, and the loading screen closes 8 frames later. A type 7 that arrives earlier creates the actor without the event (before `scene_load_scene` sets the current scene, `scene_is_in_world` is false), or fires the event while the screen is still loading, where it is ignored. Either way the loading screen never closes, and a later type 7 for the same id only resets the actor's properties. | **Observed** (bytecode), **Inferred** (effect) | S6 `0`, `0.9`, `0.30`; S7 `0.61`; S3 `0.33`; S1 `0.19`, `0.37`; S2 `0.9` |
| 1.2 | **Type 8 must come after type 7.** `actor_system_enter_finish` acts only on an actor that exists. For the local id it calls `net_manager_enable_send`, which turns type-0 sends back on and flushes the cache in order. Without it the client's script messages stay cached for good: 772 and the local player's entry both turn sending off with caching. | **Observed** (bytecode), **Inferred** (effect) | S3 `0.45`, `0.53`; S2 `0.31`, `0.32`; S1 `0.56` [178]–[180] |
| 1.3 | **Which sends wait.** In the world, script messages to the game go as type 0 (`net_manager_send`, `0.25`), which holds them while sending is off and caches them while caching is on. `send_gateway` (0xFF, `0.22`), `send_enter` (`0.26`), `send_leave` (`0.27`) and `send_time_sync` (`0.28`) check only for `WORK`. So type 2 and type 3 go out while type-0 sends are off, and 392 `PROTOC2S_CLIENT_EXIT`, which `SYSTEM.RequestQuitGame` sends through `send_gateway`, still arrives as 0xFF in the world. | **Observed** (bytecode) | S2; S14 `0.5` |
| 1.4 | **Other actors come after the player's own.** The type-7 handler drops an actor whose id isn't the local one while no local player exists, with an error log. | **Observed** (bytecode) | S2 `0.9` [57]–[66] |
| 1.5 | **Back to the select screen** is outside slice 1. `SCENE.RequestLogout` (`0.62`) sends type 4 with an empty body, after turning sends off; the handler that returns the client to the select screen is 330 `PROTOS2C_ACTOR_LOGOUT`. | **Observed** (registration), **Inferred** (that 330 answers 4) | S7 `0.62`; S2 `0.27`; S1 `0` [120]–[123] |

## 2. 772 `PROTOS2C_ACTOR_LOAD_SCENE`

`scene_manager.luac` registers `0.56` for 772 with `net_manager_force_set_stub`, beside 92, 330, 394, 490 and 733 (S1 `0` [105], [112]–[135]). Its four arguments:

| Argument | Type | Use | Required? | Fenji sends | Tag | Evidence |
|---|---|---|---|---|---|---|
| actor id | integer above 0 | `actor_system_set_local_id` [155]–[157]: the id the type-7 handler, `time_manager_update` and the native move code (`move_set_local_actor_id`, a `u64`) compare with | required | the pid | **Observed** (bytecode), **Inferred** (meaning) | S1 `0.56`; S3 `0.42`; X9 |
| `sinfo.scene_uid` | number | `math.floor(tonumber(…))`, the instance id (`GetUID`) | required: `nil` raises in `tonumber` | the scene's tid | **Observed** | S1 `0.56` [52]–[58] |
| `sinfo.scene_tid` | number | the same, the scene: the `maps` row and the load (2.5) | required | the scene | **Observed** | S1 `0.56` [59]–[65], `0.51` |
| `sinfo.scene_lv` | number | the same, the instance difficulty (`GetInstanceDifficulty`) | required | 0 | **Observed** (use), **Inferred** (meaning) | S1 `0.56` [66]–[72] |
| `sinfo.scene_type` | any | copied; `scene_get_type` returns it or 0; 0 and 4 count as the open world, and 6 forbids teleports | optional | 0 | **Observed** (copy), **Inferred** (meanings) | S1 `0.56` [73]–[74], `0.5`, `0.18`, `0.37` |
| `sinfo.is_pvp`, `is_switch`, `is_load` | any | copied; `is_load` is only logged | optional | omitted | **Observed** | S1 `0.56` [75]–[85] |
| position | a table `{x, y, z, yaw}` | kept as given; `[1]`–`[3]` place the camera 180 units above the point, and the native `scene_start_load` reads it (2.8) | required | four floats | **Observed** (uses) | S1 `0.37`, `0.47`; X7 |
| properties version | integer | compared with the scripts' constant 4706187382722034291 | send the constant | 4706187382722034291 | **Observed** (bytecode) | S1 `0.56` [29]–[34] |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | **One load at a time.** While a scene loads during a replay, the handler keeps its arguments for later. Otherwise it asserts that no load is under way, so a second 772 during a load raises (the dispatcher catches it, [`lobby.md`](lobby.md) 2.2) and is lost. | **Observed** (bytecode), **Inferred** (effect) | S1 `0.56` [1]–[28] |
| 2.2 | **The version check only logs.** A fourth argument other than the constant logs an error and the handler carries on. That the constant is `ks.pickle_hash` of the 2,439 property names, which `client_actor_class_register` computes as `__ACTOR_PROPERTIES_VERSION`, is **Inferred**; nothing else reads the argument. | **Observed** (the comparison), **Inferred** (the rest) | S1 `0.56` [29]–[34]; S8 `0` |
| 2.3 | **A first entry reloads the game.** In the login stage with no current scene, the handler calls `game_system_try_reload()`, which unloads every add-on, reloads the game's modules and starts the root add-ons. Unloading the select screen destroys its preview actor, whose id is the pid, before the handler sets the local id, so nothing stands in the way of the world's actor. | **Observed** (bytecode), **Inferred** (effect) | S1 `0.56` [37]–[50]; S10 `0.0`, `0.3`, `0.21`, `0.13` |
| 2.4 | **The scene's record.** The handler builds `new_info` from the scene table, the position, the previous scene and the `maps` row of the tid (`0.51`): its `name`, `resoureName`, `goal_progress`, `hmap_load_range` and `view_range_factor`, the last three 1 when missing. An unknown tid gives placeholder names and an error log. | **Observed** | S1 `0.56` [51]–[120], `0.51` |
| 2.5 | **Whether the map loads** (`cli_loadmap`, `0.54`): always for tid 1906. With no current scene, as on a first entry, it loads exactly when the `maps` row has a `resoureName`. Later entries skip the load for the same tid, and for the same map within 50,000 units of the last position. When nothing loads, `cur_info` takes the new scene at once. So a first entry's scene must be a `maps` row with a `resoureName` whose map ships with the client; scene 5's row names map `g10`, which ships (L4). | **Observed** (bytecode), **Inferred** (the requirement) | S1 `0.54`, `0.8`, `0.13`, `0.18`; D3; L4 |
| 2.6 | **Then** the handler sets the local id, moves an existing local player when nothing loads, turns type-0 sends off with caching (`net_manager_disable_send(true)`), and fires `SCENE.EVENT_LOAD_SCENE(tid, cli_loadmap, replaying)`. | **Observed** | S1 `0.56` [155]–[186] |
| 2.7 | **The load.** `OnRequestLoadScene` (`0.24`) shows the loading screen for the tid and unloads the login add-ons. `UpdateLoadScene` (`0.9`) calls `scene_load_scene` (S1 `0.37`: the new scene becomes current, the camera goes to the point), then each frame `scene_start_load` once and `scene_update_load` for the progress (S1 `0.47`). The game's add-ons load from 50 %, and at 100 % with every add-on loaded, `SCENE.NotifySceneLoaded()` sends type 3. Nothing in this path waits for the server. | **Observed** (bytecode), **Inferred** (the flow) | S6 `0.24`, `0.9`; S1 `0.37`, `0.47`; S7 `0.61` |
| 2.8 | **Positions are floats natively.** `scene_start_load` (FUN_140227530) reads the position through FUN_1402463C0 and FUN_1400E87E0, which make a `Float4` of four `f32` from a table (keys 1–4, or x, y, z, w, `fX`, `fY`, `fZ`, `fYaw`; integers or floats) or take a `Float4` as it is (FUN_1400E8790, FUN_1400E8140). That x, y and z are centimetres with z up, and the yaw degrees, is **Inferred** (world spec §2). | **Inferred** | X7 |

## 3. Type 3 `CTS_PROTOCOL_CLIENT_ENTER`

`send_enter` (FUN_140221BB0) posts a message of type 3 with a 21-byte body (0x15 at the command's +0x18). The script sender passes it no arguments (S2 `0.26`), so nothing in it comes from the game's state.

| Offset | Bytes | Content | Source | Tag |
|---|---|---|---|---|
| 0 | 1 | 1 | a constant | **Observed** |
| 1 | 4 | 0 | FUN_140246E90 writes 0 to both its outputs | **Observed** |
| 5 | 4 | 0 | the same | **Observed** |
| 9 | 4 | the Windows version, `(major << 8 \| minor) << 16 \| build`, or 0 if `RtlGetVersion` fails | FUN_1402481D0 | **Inferred** |
| 13 | 4 | 0 | FUN_1402481D0's second output, always 0 | **Observed** |
| 17 | 1 | 1 if the image name of the client's parent process is `Client.exe` (any case), else 0 | FUN_141156EA0, FUN_141156CC0 | **Inferred** |
| 18 | 3 | 0 | | **Observed** |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 3.1 | **The layout above,** from FUN_140221BB0's command: type byte 3, length 21, then the five 4-byte words and the byte the table lists. The runs' bodies were 21 bytes, with 1 and the zeros where the table has them (§13). | **Inferred**; constants **Observed**; the length and the constant bytes **Observed** (runs) | X2; §13 |
| 3.2 | **Fenji reads nothing in it.** Type 3 means "loaded". The Windows version and the parent flag are facts about the player's machine, so Fenji neither logs nor keeps them; its decoder checks the length only. At trace level the link logs the body with those five bytes masked (`..`), so a run's log shows the rest without them. | the choices are Fenji's | 3.1 |

## 4. Type 7 `STC_PROTOCOL_SYNC_ENTER` and the property blob

| Offset | Bytes | Field | Pushed to the scripts as | Tag |
|---|---|---|---|---|
| 0 | 8 | actor id, `u64` LE | an integer, all 64 bits | **Inferred**; offsets **Observed** |
| 8 | 1 | actor type (1 player, 2 NPC, …) | an integer | the same |
| 9 | 1 | difficulty | an integer | the same |
| 10 | 4 | template id, `u32` LE | an integer | the same |
| 14 | the rest | the property blob | a full userdata of `length − 14` bytes | the same |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.1 | **The header.** FUN_140222E00's case 7 (0x140222FAE–0x140223001) copies the body from offset 14 into a new userdata, then pushes the id (`MOV RDX, qword`), the two bytes zero-extended and the `u32` zero-extended. The handler therefore receives (blob, id, type, difficulty, template). The length isn't checked: a body under 14 bytes would underflow, so the server never sends one. | **Inferred**; offsets **Observed** | X1; S2 `0.9` |
| 4.2 | **Creating the actor** (S3 `0.33`): `lua_actor_new(id)` (FUN_1401AF2F0 reads the id with Lua's integer check); the base properties 1098 = type, 88 = difficulty and 1099 = template; the NPC tables, for type 2 only; then `lua_actor_props_unpack(actor, blob, create_handles, pre_unpack)`. Then, if `scene_is_in_world()`, it fires `SCENE.EVENT_ACTOR_ENTER` and, for the local id, `actor_system_local_player_enter` (`0.53`). | **Observed** (bytecode) | S3 `0.33`, `0.53`; X10 |
| 4.3 | **`lua_actor_props_unpack`** is FUN_1401AF730 (bound at 0x1401AFCDC). It runs `pre_unpack`, which writes the base properties, then decodes the blob inside a protected call (FUN_1400E2D60, `lua_pcallk`, with an error handler in the blob's stack slot), then runs `create_handles`, then notifies the properties that changed. A blob whose first two bytes are `FF FF` goes to another decoder (FUN_1401ACE70) for a named-property form, which Fenji doesn't use. | **Inferred** | X4 |
| 4.4 | **The blob** (FUN_1401ACD10, FUN_14017BA60) is a run of entries with no count and no terminator, read to its end. Each entry starts with a `u16` LE whose low 12 bits are the property index and top 4 bits a tag; the tag's data follows (table below). Index 0, tag 15, or an entry cut short ends the blob quietly. | **Inferred**; the tags **Observed** (the jump table at 0x14017BD64) | X4 |
| 4.5 | **Errors.** An index above the class's property count (2,439) raises an out-of-range error, a pickled value that doesn't unpack to exactly one value raises an unpack error, and a value the property's setter refuses raises a set error. Each ends the protected call: the properties after it in the blob are never set, but the actor is still created and enters. | **Inferred** | X4, X5 |
| 4.6 | **Strings carry one byte after them.** For tags 9–11 the reader writes 0 over the byte after the string and moves past it (0x14017BC46, 0x14017BC88), so an encoder writes a NUL there; without it, the next entry would lose its first byte. | **Observed** (instructions) | X4 |
| 4.7 | **Pickled values** (tags 12–14) hold a packet in the client's pickle ([`../payloads.md`](../payloads.md)), which must unpack to exactly one value (FUN_1400D5850); the setter then gets it as a Lua value. Tables travel this way. | **Inferred** | X4 |
| 4.8 | **The property table is fixed in the exe.** BUILTIN_PROPS (FUN_1401AC960, built from 24-byte entries at 0x143105350 ending at index 0) lists all 2,439 properties: index, name and a type. `client_actor_class_register` merges each `prop_tables` row into the entry of the same name, which keeps its index (S8 `0`). Its index for every name equals the one the design survey computed from `prop_tables` sorted by sync class (L2, L4), so the scripts' numeric constants (1 name, 16 position, 222 equips, 1325 speed, …) are those indexes. | **Observed** (the table), **Inferred** (the merge) | X6; S8 `0`; L2 |
| 4.9 | **Which properties the blob may set.** `client_actor_class_register` registers every property of type `bool`, `int`, `num`, `vec`, `str` and `locale_str` with its setter switched on (the third argument `true`), and `meta`, `table` and `any` with Lua setters (`xtype_set_lua_value`). A row whose sync is `server` gets the type `""`, and it and the `templ` types get `xtype_set_nil_value`, whose setter returns 0 (FUN_14017BDB0, FUN_140097CA0): sending one ends the blob (4.5). The sync class doesn't otherwise limit the blob: `prop_move_speed` (1325, sync `""`, `int`) is settable. | **Observed** (bytecode, the setters), **Inferred** (effect) | S8 `0` [275]–[455]; X5 |
| 4.10 | **What each type accepts** (table below). A value of the wrong kind for its property raises 4.5's set error. | **Inferred** | X5 |
| 4.11 | **Unset table properties read as an empty table:** the `table` type's getter (S8 `0.5`) creates `{}` on its first read. | **Observed** (bytecode), **Inferred** (effect) | S8 `0.5`, `0.6` |
| 4.12 | **Vectors are four `f32`.** Tag 8's 16 bytes are copied into the property (FUN_14017DA60), which the `vec` getter returns as a `Float4` (x, y, z, yaw), and `Float4`s hold `f32` (FUN_1400E8140). | **Inferred** | X5, X7 |

| Tag | Kind | Data after the header |
|---|---|---|
| 0 | nil | none |
| 1 | `false` | none |
| 2 | `true` | none |
| 3 | integer | `i8` |
| 4 | integer | `i16` LE |
| 5 | integer | `i32` LE |
| 6 | integer | `i64` LE |
| 7 | number | `f64` LE |
| 8 | vector | four `f32` LE: x, y, z, yaw |
| 9 | string | `u8` length, the bytes, a NUL |
| 10 | string | `u16` LE length, the bytes, a NUL |
| 11 | string | `u32` LE length, the bytes, a NUL |
| 12 | pickled value | `u8` length, then a pickle packet of one value |
| 13 | pickled value | `u16` LE length, then the packet |
| 14 | pickled value | `u32` LE length, then the packet |
| 15 | none: ends the blob | |

| Property type | Accepts | Setter |
|---|---|---|
| `int` | integer; `true`/`false` as 1/0; a number, truncated; a string, through `strtoll` | FUN_14017BED0 (unbounded), registered by FUN_14017C990 |
| `num` | number; integer; `true`/`false` as 1/0; a string, through `strtod` | FUN_14017CBA0, registered by FUN_14017D830 |
| `bool` | `true`/`false`; nil as false; a non-zero integer, number or numeric string as true | FUN_14017BDF0 with FUN_14017B4C0 |
| `str` | string; nil as ""; `true` as "1"; integers and numbers printed | FUN_14017DB20, registered by FUN_14017DD30 |
| `vec` | tag 8; a pickled table or `Float4`, converted | FUN_1401B1E40, FUN_14017DA60 |
| `locale_str` | a string, passed to the script's setter, which unpickles one that starts with a fixed prefix and keeps any other as it is | FUN_1401B0470 with S8 `0.8` |
| `table`, `any` | any value, kept by the script's setter (tables arrive pickled) | FUN_1401B0910 with S8 `0.6`, `0.10` |
| `""` (sync `server`), `templ…` | nothing | FUN_140097CA0 |

## 5. The player's own actor

What Fenji puts in the player's own blob, in index order. Types and defaults are `prop_tables`' (D2).

| Index | Property | Type | Value | Why | Tag |
|---|---|---|---|---|---|
| 1 | `prop_base_name` | `locale_str` | the name | the name board and HUD (`GetName`) | **Inferred** |
| 4 | `prop_base_gender` | `int` | 0 for skeleton 1, else 1 | the model, as the lobby derives it ([`lobby.md`](lobby.md) §4) | **Inferred** |
| 5 | `prop_base_vocation` | `int` | the vocation | the model; the default −1 breaks it | **Inferred** |
| 6 | `prop_base_skeleton` | `int` | the skeleton | the model; the default 0 matches no skeleton | **Inferred** |
| 9 | `prop_base_lv` | `int` | the level, 37 at creation | the HUD (`GetLevel`) | **Inferred** |
| 16 | `prop_base_position` | `vec` | x, y, z, yaw, as in 772 | where the actor stands | **Inferred** |
| 105 | `prop_sys_portrait` | `int` | the stored portrait | the head icon | **Inferred** |
| 109 | `prop_sys_show_equip_head` | `bool` | `false` | the outfit's head hidden (§9) | **Inferred** |
| 128 | `prop_sys_result_show_equip_head` | `bool` | `false` | the model reads the result flags | **Inferred** |
| 177 | `prop_sys_player_voice_result` | `int` | the stored voice, when there is one | the model's `SetVoiceIndex` | **Inferred** |
| 214 | `prop_sys_avatar_feature` | `int` | the base face | the model | **Inferred** |
| 215 | `prop_sys_avatar_feature_face` | `str` | the face string, when there is one | the model | **Inferred** |
| 222 | `prop_sys_equips` | `table` | the creation outfit (§9), pickled: slot → item id | the model | **Inferred** |
| 300 | `prop_num_cur_hp_rat` | `num` | 1.0 | `GetHPRat` (5.6) | **Inferred** |
| 304 | `prop_num_result_hp_max` | `int` | `FULL_HEALTH`, 1000 | `GetHPMax`, `OnHPChanged` (5.6) | **Inferred** |
| 1325 | `prop_move_speed` | `int` | the speed rate, 100 | walking (§6) | **Inferred** |
| 1458 | `prop_num_cur_hp` | `int` | `FULL_HEALTH`, 1000 | `GetHP`, and alive: at 0 the client draws the player dead (5.6) | **Observed** (run m4-e1), **Inferred** |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 5.1 | **Leaving the loading screen needs no property:** any type 7 for the local id after type 3 does it (1.1). | **Inferred** | 1.1 |
| 5.2 | **Standing where the world says needs 16:** nothing else places the player's actor. 772's position moves only an actor that already exists (2.6), and the local player's entry hooks move nothing (S1 `0.49`). Without 16 the actor keeps its zero vector. | **Inferred** | S1 `0.56`, `0.49`; S3 `0.53` |
| 5.3 | **The look.** The client's own recipe for a player model is the select screen's preview: properties 4, 5, 6, 214, 215, 222, 166, the result flags 128–140 (from the lobby's show flags), 126, 127, 145 and 148, created with type 1, difficulty 255 and template `vocation × 10000` (S10 `0.0` [20]–[123]). The model's init reads 5, 4, 6, 222 and the result flags, and 177 sets its voice (S11 `0.26`). In the world every result flag but 143 (dress) defaults to `true` (D2), so Fenji sends only the flags it hides. | **Observed** (bytecode, defaults), **Inferred** (enough) | S10 `0.0`; S11 `0.26`; D2 |
| 5.4 | **The header Fenji sends:** type 1, difficulty 255 and template `vocation × 10000`, as the preview's. For players the scripts read the difficulty and the template only as properties 88 and 1099; the NPC tables use them for type 2 only. | **Inferred** | S3 `0.33`; S10 `0.0` |
| 5.5 | **Chat's tables needn't be sent.** Unset, 389 (`prop_sys_chatbox_setting`) and 396 (`prop_sys_emoji_infos`) read as `{}` (4.11). The chat settings then fall back to their defaults, which the client saves with 422, and no emoji counts as unlocked. Nothing fails. Fenji may send them as pickled tables later. | **Inferred** | 4.11; L4 (chat survey §4, §10) |
| 5.6 | **Health must be sent.** Left at their defaults (304 `prop_num_result_hp_max` 1, 300 and 1458 0), run m4-e1's HUD showed "HP 0" and the whole scene grey. The player model's setup registers `OnHPChanged` for 1458 and 304 and calls it once (S17 `0.4` [6]–[24]). It sets the model's health target to 1458 ÷ 304 when 1458 is 0, and to 1 otherwise (`0.20` [9]–[15]). While that target is at most 0.6, the model's update turns on the `PostDead` post-effect with a saturation of target ÷ 0.6 (`0.21` [173]–[266]), so a current health of 0 draws the scene without colour. The scripts read 300, 1458 and 304 through `GetHPRat`, `GetHP` and `GetHPMax` (S18 `0.47`, `0.48`, `0.50`). Death itself is property 15 `prop_base_dead`, which defaults to 0. So Fenji sends full health: 300 = 1.0, and 304 and 1458 = `FULL_HEALTH` (1000; M4 has no combat, and the value is Fenji's choice). | **Observed** (bytecode; run m4-e1), **Inferred** (that 1458 caused the grey) | S17; S18; D2; M4E1 |

## 6. Type 8, and the speed

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 6.1 | **Type 8's body is the actor id,** a `u64` LE (FUN_140222E00's case 8, which case 10 falls into after pushing its blob). The handler calls `actor_system_enter_finish(id)` (1.2). | **Inferred**; offset **Observed** | X1; S2 `0.10`; S3 `0.45` |
| 6.2 | **Type 23 `SYNC_PROP_FORCE`** (FUN_14020CE90): a `u64` actor id, a `u16` property index and an `i16` value, 12 bytes. Index 1325 sets `prop_move_speed` and 1330 `prop_move_gravity` (FUN_140211630, FUN_1402114E0, each through the property's own setter, the one the blob uses); other indexes are ignored. When a flag in the actor's move state is set, it also clears a value of the move code (DAT_1431F37A8; both meanings **Unknown**). The actor must exist (FUN_1401AD3A0 finds it by its integer id), so 23 comes after 7. | **Inferred**; 0x52D and 0x532 **Observed** | X8 |
| 6.3 | **Type 21 `MOVE_BASE_PROP`** (FUN_14020BA70): a `u64` actor id, a `u64`, three `i32`, an `i16`, then the speed and the gravity as `i16`, 34 bytes. It also writes the middle fields into the actor's move object (meaning **Unknown**), and a short body leaves stale values in them. So Fenji uses 23. | **Inferred** | X8 |
| 6.4 | **The rate.** `move_get_move_speed_by_rat` (0x14020DE10, bound at 0x14020AAC0; Ghidra's analysis made no function there) multiplies the rate by 500.0 and by 0.01 (the `f32` constants at 0x142F04890 and 0x142F046B8): rate 100 is 500 units. The scripts' logic speed is `prop_move_speed` less an offset for backward moves, and a rate below 60 picks walking (L4). That the 500 is per second is **Inferred**. | **Observed** (bytes), **Inferred** (per second) | X9; L4 |
| 6.5 | **Unset, the speed is 0** (D2 gives no default), and the player can't move. Fenji sets it in the type-7 blob, so it is in place before the move handle exists (4.3). The plan also sent type 23 until Task 11's runs: with both, the character ran about 5 m/s (m4-e2), and without type 23 it ran the same stretch as fast (m4-e3), so Fenji sends the blob's 1325 alone and keeps type 23 for later changes of speed. | **Observed** (default; runs m4-e2, m4-e3), the choice is Fenji's | D2; 4.3; 6.2; §13 |

## 7. Time syncs (type 2)

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 7.1 | **When.** The main loop calls `time_manager_update` every frame (S5 `0.9`). It returns while `actor_system_get_local_id()` is 0 or less (S4 `0.1` [4]–[8]); the id starts at 0 (S3 `0` [204]–[216]), and 772 sets it (2.6). Then the first sync goes out at once and one every 1,000 ms (`SERVER_TIME_SYNC_INTERVAL`), through `net_manager_send_time_sync` (`0.28`), which checks only for `WORK`. So the syncs start right after 772, during the loading screen, before the actor exists. In the lobby the id stays 0 and no sync is sent, as the M2 and M3 runs saw. | **Observed** (bytecode) | S4 `0`, `0.1`; S5 `0.9`; S2 `0.28`; S3 `0`, `0.42`, `0.82` |
| 7.2 | **The client's body,** 13 bytes (FUN_140221AC0): the low 32 bits of its Unix-millisecond clock (FUN_1400B5670), a byte 1, and two `u32` the client fills in itself: one read from its own module's export `ipvtmeg` (FUN_140246E40), and one returned by the function its export `curl_easy_paste` points to (FUN_140246DF0). Both are counters the client keeps about its own machine (FUN_140A9D6A0, FUN_141156C20, FUN_140A9D650). Fenji reads neither, and its trace log masks them. In the runs the body was 13 bytes with the byte 1, and the `u32` equalled the low 32 bits of the Unix time in ms at the server's receipt (§13). | **Observed** (the length, the clock and the byte 1: runs); **Inferred** (the counters); the exports **Observed** (the PE export table) | X3; L1; §13 |
| 7.3 | **The reply's body,** 12 bytes (FUN_140222E00's case 2): a `u32`, the client's value echoed back, and a `u64`, the server's time in milliseconds. Natively, when the `u32` is below the client's clock, both as unsigned 32-bit numbers, the client updates a smoothed round trip (7/8 old, 1/8 new) and its server-time estimate: the `u64` plus half the round trip. `net_client_get_server_time` (FUN_140222DE0) returns the local clock plus the estimate's lead over it, which is the client's own clock until a reply comes. | **Inferred** | X1; X10 |
| 7.4 | **The scripts' handling.** The type-2 handler (`0.7`) calls `time_manager_time_sync_resp(now, u32, u64)` (S4 `0.2`). It returns at once when `now − u32` is negative. Otherwise it sets `net_delay` to `now − u32` and `server_time_now` to `u64 + net_delay // 2`, Lua's floor division of integers. | **Observed** (bytecode) | S2 `0.7`; S4 `0.2` |
| 7.5 | **So Fenji answers each type 2 at once** with the client's `u32` unchanged and its own time in milliseconds since the Unix epoch: the epoch the client's clock and its no-reply estimate already use (7.3). | **Inferred** | 7.3, 7.4 |

## 8. Actor ids

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 8.1 | **Ids are 64-bit natively.** Types 7 and 8 read `u64` ids and push them as Lua integers (X1); `lua_actor_new` reads its id with Lua's integer check (FUN_1401AF2F0); `move_set_local_actor_id` takes a `u64` (its signature `Q` at 0x141189F00) and stores all 64 bits (0x140210A30); actors are found by integer key (FUN_1401AD3A0). | **Inferred**; widths **Observed** | X1, X8, X9, X10 |
| 8.2 | **The NPC rule.** `actor_id_enter` treats an id whose `id >> 48` is above 0 as an NPC's (S9 `0.13` [1]–[3]), and 0 means "no actor" (S3 `0.39`; S4 `0.1`). So a player's id is in 1 to 2^48 − 1, as `MAX_PLAYER_ID` in `fenji-server` has it. | **Observed** (operands) | S9 `0.13`; S3 `0.39`; S4 `0.1` |
| 8.3 | **So a pid above 2^32 survives** the native paths and the scripts' integer paths, and since every player id is below 2^53, it survives a conversion to a float too. Run m4-e4 entered and ran with pid 4294967297 (2^32 + 1; §13). | **Inferred**; the run **Observed** | 8.1, 8.2; §13 |

## 9. The creation outfit

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 9.1 | **The client doesn't fill the outfit in.** `prop_base_equips_id` appears in the scripts only in the create panel's 304; no script reads `CteateEquips`; `vocation.luac` reads only `PreviewEquips`, for the create screen (`0.20`); and the exe holds no `equips_id` string. The world's model takes the outfit from property 222 alone (S11 `0.26`). So the server sends 222, with item ids. | **Observed** (absences, bytecode), **Inferred** (conclusion) | S12; S11 `0.26`; X11 |
| 9.2 | **The ids** are `vocation_initequip_templ.CteateEquips[vocation][outfit]`, outfits 1 and 2 (the create screen's two buttons; the Western screen shows only 2), as the table below. The creation template hides the head: `CreateShowEquipHead` is 0 and `creator_info.prop_sys_show_equip_head` is 0 for every vocation (D3). No `CteateEquips` entry for outfits 1 and 2 has a head (slot 1) or a belt (slot 4). | **Observed** (the sheet) | D3; L3; L4 |
| 9.3 | **The table's form.** Keys are the `EQUIP_SLOT_*` numbers (1 head, 2 chest, 3 wrist, 4 belt, 5 trousers, 6 boots, 11 weapon; S13), and values item ids `type << 24 \| id`, type 5 a weapon and 6 armour (S15). The model looks each slot up with an integer key, so the pickled table's pairs may come in any order. | **Observed** (constants), **Inferred** (the rest) | S13; S15; S11 `0.26` |
| 9.4 | **Vocation 8** is hidden on the Western create screen, has no trousers in either outfit, and starts in a scene whose map doesn't ship (L4). | **Observed** (sheet), **Inferred** | D3; L4 |

| Vocation | Outfit | 2 chest | 3 wrist | 5 trousers | 6 boots | 11 weapon |
|---|---|---|---|---|---|---|
| 0 | 1 | 100667850 | 100667855 | 100667870 | 100667860 | 83886281 |
| 0 | 2 | 100664357 | 100664363 | 100664381 | 100664369 | 83886281 |
| 1 | 1 | 100667851 | 100667856 | 100667871 | 100667861 | 83886282 |
| 1 | 2 | 100664358 | 100664364 | 100664382 | 100664370 | 83886282 |
| 2 | 1 | 100667852 | 100667857 | 100667872 | 100667862 | 83886283 |
| 2 | 2 | 100664359 | 100664365 | 100664383 | 100664371 | 83886283 |
| 3 | 1 | 100667853 | 100667858 | 100667873 | 100667863 | 83886284 |
| 3 | 2 | 100664360 | 100664366 | 100664384 | 100664372 | 83886284 |
| 4 | 1 | 100667853 | 100667858 | 100667873 | 100667863 | 83886285 |
| 4 | 2 | 100664361 | 100664367 | 100664385 | 100664373 | 83886285 |
| 5 | 1 | 100667854 | 100667859 | 100667874 | 100667864 | 83886286 |
| 5 | 2 | 100664362 | 100664368 | 100664386 | 100664374 | 83886286 |
| 6 | 1 | 100667850 | 100667855 | 100667870 | 100667860 | 83886287 |
| 6 | 2 | 100665874 | 100665876 | 100665880 | 100665882 | 83886287 |
| 7 | 1 | 100667853 | 100667858 | 100667873 | 100667863 | 83886288 |
| 7 | 2 | 100665875 | 100665877 | 100665881 | 100665883 | 83886288 |
| 8 | 1 | 100670186 | 100670187 | — | 100670190 | 83886289 |
| 8 | 2 | 100670186 | 100670187 | — | 100670190 | 83886289 |
| 9 | 1 | 100670202 | 100670207 | — | 100670222 | 83886287 |
| 9 | 2 | 100665874 | 100665876 | 100665880 | 100665882 | 83886287 |

The item ids are documented interoperability data, by the owner's decision of 2026-10-13 (the M4 world spec §3.2, #180): a host's server can't read the client's compressed tables, so Fenji ships these ids.

## 10. Closing a player's connection

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 10.1 | **What the player sees.** A type-1 message closes the connection, its body becoming the close reason ([`../transport.md`](../transport.md) 7.4). In the world, UIMain's net-status timer (`0.3`) then shows the text `NET.FetchNetErrorByErr(reason)` finds for state 5, or a generic text about a server or connection failure when it finds none; the reason `client-exit` makes the client quit instead. A connection the server just drops ends in state 4 once the client's next send fails ([`../transport.md`](../transport.md) 1.4–1.5), and for state 4 the timer shows a fixed text about server maintenance. | **Observed** (bytecode), **Inferred** (the states) | S6 `0.3`; [`lobby.md`](lobby.md) §9; D4 |
| 10.2 | **The keys the link can use** ([`lobby.md`](lobby.md) 9.3): `error duplicate-login`, M3's choice for a login that a newer one replaced; `error server-not-running`, whose text says the server is unavailable for now; `error kick-by-gm`, whose text says suspicious activity was found on the account; and `error kick-by-user`, whose text says the player is offline and may log in again. | **Observed** (keys; D4's English texts) | S16; D4 |
| 10.3 | **Fenji's choices:** a replaced login gets `error duplicate-login`; a server stopping gets `error server-not-running`; an admin's `!kick` gets `error kick-by-user`, which accuses the player of nothing, unlike `error kick-by-gm`; a client that stops reading or floods gets no reason, so it sees 10.1's maintenance text. | the choices are Fenji's | 10.1, 10.2 |

## 11. What Fenji sends

| The client sends | Fenji answers | Values |
|---|---|---|
| 10, a pid of the account's | 772 | `(772, pid, {scene_uid = scene, scene_tid = scene, scene_lv = 0, scene_type = 0}, {x, y, z, yaw}, 4706187382722034291)`, the position as floats |
| type 2 | type 2 | the client's `u32`, and the server's Unix time in ms (§7) |
| type 3 | type 7, type 8 | the header `(pid, 1, 255, vocation × 10000)` and §5's blob, with the speed and full health; then `pid` |
| type 3 of any length but 21 bytes | closes the connection | without a reason; only the length is checked (3.2) |
| the cached type-0 script messages | nothing | none needs an answer in slice 1 |
| 392 (0xFF) | closes the connection | as in the lobby |
| type 4 | closes the connection | without a reason or 330, so the client doesn't go back to the select screen: that is outside slice 1 (1.5) |

**Golden vector** of a type-7 body, for the character M3's run created (Lin, vocation 7, skeleton 3, level 37, portrait `169 << 16`, voice 3, base face 1, outfit 2), with a short face string `v:5;` in place of its real one, pid 258, the default start (scene 5, `2366, −71735, 64854, 345`), a speed of 100 and full health. It is 141 bytes: the 14-byte header, then the entries of §5's table in index order.

```
02 01 00 00 00 00 00 00 01 ff 70 11 01 00
01 90 03 4c 69 6e 00
04 30 01   05 30 07   06 30 03   09 30 25
10 80 00 e0 13 45 80 1b 8c c7 00 56 7d 47 00 80 ac 43
69 50 00 00 a9 00
6d 10   80 10
b1 30 03   d6 30 01
d7 90 04 76 3a 35 3b 00
de c0 2a 01 0b 00 00 00 00 03 02 05 13 0a 00 06 03 03 05 15 0a 00 06
         03 05 05 19 0a 00 06 03 06 05 1b 0a 00 06 03 0b 05 d0 00 00 05 00
2c 71 00 00 00 00 00 00 f0 3f   30 41 e8 03
2d 35 64
b2 45 e8 03
```

The other bodies' vectors are in the [world plan](../../superpowers/plans/2026-10-13-m4-world.md)'s Amendment A.

## 12. Open questions for the runs

Only a run of the real client can settle these. Task 11's runs (`scripts/spike/run-client.sh` with `--server`) answer them; each run's prediction is written before it starts. Those runs answered 1–6 (§13); 7 and 8 are still open.
1. **The sequence works:** after 10, the loading screen for scene 5 closes and the character stands at 772's position; the server's log shows type-2 syncs from 772 on, then type 3, then the client's flushed script messages after type 8. Task 11's first world run.
2. **The character walks** with `prop_move_speed` 100, at about 500 units a second, and whether the blob or type 23 alone would do (6.5). The same run, walking for a timed stretch.
3. **The look:** the face, the voice and the outfit as at creation, with the head hidden (5.3). The run's screenshots.
4. **The HUD's health bar** with 300, 304 and 1458 at their defaults (5.6). The screenshots.
5. **Units:** that 772's x, y and z are centimetres with z up and the yaw degrees, and whether the client puts the character on the ground when the start's z is above it (2.8). The run's position reports.
6. **A pid above 2^32** (8.3). A run with a character whose pid is seeded above 2^32.
7. **Two clients in one Wine prefix** (world spec open question 7). The exe imports `CreateMutexW`, `OpenMutexW` and `CreateMutexA`, whose uses weren't traced. The two-client run.
8. **The type-2 reply's effect:** that dates the client shows follow the server's time. The run, with the client's clock set away from the host's.

## 13. Runs

Task 11's runs (2026-10-14) ran the pinned client in `scripts/spike/run-client.sh` with `--server`, `--launch`, `--click`, `--key` and `--type`, against `fenji-server` built from the task's branch, with the AGT driver watchdog armed. Each run:
- started a new `fenji-server` on one database (`re/runs/server-data`), which held the account `tester`, made on the host through the API before the first run;
- ran `fenji-launch play --print` inside the sandbox once the server was up, and gave the client only the launcher path's three pairs, with a fresh ticket;
- gave the client `--fs=data:_index:_index` and no `--gf-sdk` flag, with `[world]` at its defaults.

Clicks and keys went through XTEST on the nested display, and screenshots after the runner's last one (295 s) were taken on the host. The server logged at trace what it received, with type 3's and the time syncs' machine bytes masked (3.2, 7.2), and at debug what the link sent. Predictions were written before each run (M4R).

| Run | What the run did | What happened | Watchdog | Evidence |
|---|---|---|---|---|
| m4-e1 | creates `Lin` (Fox Mage, vocation 7) and enters; 900 s | the select screen showed Lin as "Beginner 1" in her creation outfit; 772 41 ms after 10; type 3 41.6 s later; 7, 23 and 8; the client logged "Local actor enter", and Lin stood on Blade Peak at the minimap's (23, −718). But the HUD showed "HP 0" and the scene was grey (5.6) | no sign | M4E1 |
| m4-e2 | full health; creates `Mai` (Spellsword, vocation 2) through a "+" place and enters; `g` (auto-move) for 22 s; 600 s | Mai stood in colour with a full HP bar. At `g` she ran up the stairs: the minimap went (33, −721) → (57, −726) in about 5 s, about 4.9 m/s, then she stopped against rocks | no sign | M4E2 |
| m4-e3 | as m4-e2 without type 23 (a build for this run only): Lin enters; `g`; 360 s | Lin stood in colour, and at `g` ran the same stretch, (35, −721) → (55, −724) in about 5 s, to the same rocks | no sign | M4E3 |
| m4-e4 | the final build; a new database whose next pid was 2^32 + 1; creates Lin and enters; `g`; 420 s | the select screen listed Lin, the client's 10 carried 4294967297 as an `i64`, and 772, 7 and 8 went out with it; Lin stood in colour and ran the same stretch | no sign | M4E4 |

**Observed:**
- **Entering works** (§12 question 1). 10 led to 772, the loading screen for scene 5 (the client's log: "Start load scene: 5", "Load Scene g10"), type-2 syncs from 1.5 s after 772, type 3 after 40–42 s, then the player's own actor. The client's cached script messages came 1.4 s after type 8 (1.2). A second Enter Game click during the loading screen sent nothing.
- **The default start works** for vocation 7 (m4-e1) and vocation 2 (m4-e2), so `[world]`'s defaults stay.
- **The client's bodies.** Type 3 was 21 bytes, with byte 0 = 1 and zeros where §3 reads constants. Type 2 was 13 bytes with byte 4 = 1, and its `u32` equalled the low 32 bits of the Unix time in ms at the server's receipt, in every line checked, so 7.2's clock is **Observed**. They are fixtures now, with the masked bytes zero (`crates/fenji-server/tests/fixtures/m4-e1-*.hex`).
- **Units** (question 5): the minimap's (23, −718) at the start is 772's (2366, −71735) ÷ 100, so x and y are centimetres and the minimap shows metres.
- **Health** (question 4): left at its defaults, the HUD showed "HP 0" and the client drew the scene without colour; with full health (5.6) the scene had its colour and the bar was full.
- **Walking** (question 2): from `g`, each character covered the 35 m from the start to the rocks in about 9–10 s, 3.5–5 m/s on the minimap while climbing stairs, as rate 100's 500 cm/s would (6.4); with the blob's 1325 alone as with type 23 too (6.5).
- **A pid above 2^32** (question 6): 4294967297 was listed, selected, loaded and run with (m4-e4); the client's 10 carried it as an `i64` (`06 01 00 00 00 01 00 00 00`), so it never passed through a float.
- **The look** (question 3): each character in its creation outfit and face, Lin with the Fox Mage's ears and no head item, Mai with a sword, on the select screen and in the world.
- **What the client sends in the world.** About twice a second, 97 `PROTOC2S_PLAYER_EYEPOS`. While running, native type 8 (`CTS_PROTOCOL_MOVE_DIR`, 200 bytes; 53 in 22 s). Messages its scripts call logs, among them 120, 257, 248, 540 and 560. Slice 1 ignores them all.
- **The network.** One connection, to 127.0.0.1:7800, and the sandbox's host-name lookup. On entering, the client started its own web-view helper, `browserhost.exe`, which made no connection.
- **The client's own errors.** Three listeners of `SCENE.EVENT_LOCAL_PLAYER_ENTER` failed in every run: `initFeatureMgr` ("'for' limit must be a number"), the skill panel's `GetRemnantPageState` (a nil `study`), and the action bar's `LoadActionBarSetting`. With Mai, a cooldown sweep's `SetRotation` (`interface/templates/BaseCoolDown.lua`, `UpdateProgress`) failed every 10 s on a non-finite rotation. Running into the rocks showed "Not yet learnt". None of them stopped a character standing or running.

**Inferred:**
- The listener errors, the cooldown sweep and "Not yet learnt" come from what slice 1 doesn't send yet: features, skills and the action bar's settings.

**Unknown:**
- Whether the start's z (64854) is the ground there or the client put the character on it.
- §12's questions 7 (two clients in one Wine prefix) and 8 (the reply's effect on the dates the client shows): not run.
- In m4-e4 alone, `g` also opened the World Map, and the client sent 85 `PROTOC2S_ITEM_COLLECTION_USE`; the same key in m4-e2 and m4-e3 only started the run.

## Evidence index

`re/` is git-ignored, so the script listings, caches and notes exist only on the owner's machines. `luatool` regenerates the listings from the pinned build. A script key names the disassembly `re/m1/disasm/data/<path>.luac.dis` and the decompiled `re/m1/decompiled-all/data/<path>.lua`, with the function paths cited. `PG/` stands for `data/locale/common/core/modules/pregame/`, and `AS/` for `data/locale/common/interface/addons/ActorSelect/`.

| Key | Path or address | What it holds |
|---|---|---|
| S1 | `data/core/client/scene_manager.luac` | The main chunk `0`: 772's closure into register 62 [105] and the stubs for 772, 92, 330, 394, 490 and 733 [112]–[135]. The 772 handler (`0.56`), the `maps` lookup (`0.51`), `cli_loadmap` (`0.54`), `scene_load_scene` (`0.37`), the load update that calls `scene_start_load` (`0.47`), the local player's entry hook (`0.49`), `scene_is_in_world` (`0.19`), `scene_get_type` (`0.5`), the current tid (`0.8`) and map (`0.13`), the open-world test (`0.18`) |
| S2 | `data/core/client/net_manager.luac` | The type handlers for 2 (`0.7`), 7 (`0.9`) and 8 (`0.10`); `send_gateway` (`0.22`), `send` (`0.25`), `send_enter` (`0.26`), `send_leave` (`0.27`), `send_time_sync` (`0.28`); `net_manager_enable_send` (`0.31`) and `net_manager_disable_send` (`0.32`) |
| S3 | `data/core/client/actor_manager.luac` | Creating an actor (`0.33`), the actor by id (`0.39`), `actor_system_set_local_id` (`0.42`), `actor_system_enter_finish` (`0.45`), `actor_system_local_player_enter` (`0.53`), `actor_system_get_local_id` (`0.82`) |
| S4 | `data/core/client/time_manager.luac` | `SERVER_TIME_SYNC_INTERVAL` (`0`), `time_manager_update` (`0.1`), `time_manager_time_sync_resp` (`0.2`) |
| S5 | `data/core/client/main.luac` | The per-frame function (`0.9`), which calls `time_manager_update` |
| S6 | `data/interface/system/UIMain/UIMain.luac` | The `STATE_*` constants (`0`), the net-status timer `OnTimer_CheckNetStatus` (`0.3`), `UpdateLoadScene` (`0.9`), `OnRequestLoadScene` (`0.24`), `OnLocalActorEnter` (`0.30`) |
| S7 | `data/core/modules/scene/scene.luac` | `NotifySceneLoaded` (`0.61`) and `RequestLogout` (`0.62`) |
| S8 | `data/core/xobject/client_actor_class_register.luac` | The main chunk `0`: BUILTIN_PROPS and `prop_tables` merged, the sort, `__ACTOR_PROPERTIES_VERSION`, and the registration by type [275]–[455]; the `table` getter and setter (`0.5`, `0.6`), the `locale_str` setter (`0.8`), the `any` setter (`0.10`) |
| S9 | `data/core/client/actor_id_sync.luac` | `actor_id_enter` (`0.13`): the 2^48 test |
| S10 | `PG/pregame_actor.luac`, `PG/pregame.luac`, `AS/ActorSelect.luac` | The preview actor's `create` (`0.0`): its properties and `actor_system_create_actor(pid, 1, 255, vocation × 10000, …)`; its `destroy` (`0.3`); `ClearActor` (`pregame.luac` `0.21`), which the select screen's `OnUnLoad` calls (`ActorSelect.luac` `0.13`) |
| S11 | `data/core/modules/model/player_model_handle.luac` | The model's `on_handle_init` (`0.26`), set at `0` [112] |
| S12 | `data/locale/common/interface/addons/ActorCreate/CreatePanel.luac`, `data/interface/addons/ActorCreate/CreatePanel.luac`, `data/core/modules/vocation/vocation.luac` | The only uses of `prop_base_equips_id` (the 304 request); `GetVocationPreviewEquips` (`0.20`), the one reader of the outfit sheet; a search of every listing for `CteateEquips` finds nothing |
| S13 | `data/core/share/enum_base.luac` | The main chunk `0` [1030]–[1054]: `EQUIP_SLOT_HEAD` 1 to `EQUIP_SLOT_WEAPON` 11 |
| S14 | `data/core/modules/system/system.luac` | `RequestQuitGame` (`0.5`): 392 through `send_gateway` |
| S15 | `data/core/share/item_logic/item_category.luac` | The equipment test on `id >> 24` (`0.0`) and the id from type and number, `type << 24` (`0.23`, `0.24`) |
| S16 | `data/core/modules/net/net.luac` | The close-reason table in the main chunk `0` (decompiled lines 1674–1810), as in [`lobby.md`](lobby.md) S1 |
| S17 | `data/core/modules/player/player.luac` | The player model's setup (`0.4`), `OnHPChanged` (`0.20`) and the model's update with the `PostDead` effect (`0.21`) |
| S18 | `data/core/client/actor_interface.luac` | `GetHPRat` (`0.47`), `GetHP` (`0.48`), `GetHPMax` (`0.50`) |
| D1 | `data/caches/protocol_ver.new_cache` | The protocol table: 772 `PROTOS2C_ACTOR_LOAD_SCENE`, 92, 330, 394, 10, 392 |
| D2 | `data/caches/prop_tables.new_cache` | 2,439 rows: name, type, sync class and default |
| D3 | `data/caches/all_tables.new_cache` | `vocation_initequip_templ` (`CteateEquips`, `PreviewEquips`), `vocation_templ` (`CreateShowEquipHead`, `creator_info`), `maps` |
| D4 | `data/caches/locale_en.new_cache` | The English texts of the close reasons and of UIMain's fallbacks |
| X1 | FUN_140222E00 | Inbound messages by type: case 2 (0x140222E59), case 7 (0x140222FAE), case 8 (0x14022306B), and cases 21 and 23 |
| X2 | FUN_140221BB0, FUN_140246E90, FUN_1402481D0, FUN_141156EA0, FUN_141156CC0 | `send_enter` and the parts of type 3's body |
| X3 | FUN_140221AC0, FUN_140246E40, FUN_140246DF0, FUN_140A9D6A0, FUN_141156C20, FUN_140A9D650; the exports `ipvtmeg` (0x143D27000) and `curl_easy_paste` (0x143D26000) | `send_time_sync` and the two values it reads |
| X4 | FUN_1401AF730 (bound at 0x1401AFCDC), FUN_1400E2D60, FUN_1401ACD10, FUN_14017BA60 and its jump table at 0x14017BD64, FUN_1401ACE70 | `lua_actor_props_unpack` and the blob's decoder |
| X5 | FUN_14017BED0, FUN_14017C990, FUN_14017CBA0, FUN_14017D830, FUN_14017BDF0, FUN_14017B4C0, FUN_14017BE80, FUN_14017DB20, FUN_14017DD30, FUN_1401B1E40, FUN_14017DA60, FUN_14017DAD0, FUN_1401B0470, FUN_1401A9F30, FUN_1401B0910, FUN_1401AA020, FUN_14017BDB0, FUN_140097CA0; the `xtype_set_*` bindings at 0x14116B4F0 | The setters by type and how each is switched on |
| X6 | FUN_1401AC960; the table at 0x143105350 | BUILTIN_PROPS: 2,439 entries |
| X7 | FUN_140227530 (`scene_start_load`), FUN_1402463C0, FUN_1400E87E0, FUN_1400E8790, FUN_1400E8140 | The position read as a `Float4` |
| X8 | FUN_14020CE90 (type 23), FUN_14020BA70 (type 21), FUN_140211630, FUN_1402114E0, FUN_1401AD3A0 | The speed and gravity messages |
| X9 | 0x14020DE10 (`move_get_move_speed_by_rat`, bound at 0x14020AAC0), 0x142F04890, 0x142F046B8; 0x140210A30 (`move_set_local_actor_id`, signature at 0x141189F00) | The rate's conversion; the local id's width |
| X10 | FUN_1401AF2F0 (`lua_actor_new`), FUN_140222DE0 (`net_client_get_server_time`) | The id's read; the server-time estimate |
| X11 | the exe's strings | No `equips_id` |
| L1 | `re/m4/entering/notes.md`, with its REA bundle in `re/m4/entering/rea/` | This reading's notes: every REA call and its ID, the unknowns recorded in REA, and what the local probes printed |
| L2 | `re/m4/entering/builtin_props.tsv` | BUILTIN_PROPS dumped from the exe with a standard-library PE reader |
| L3 | `re/m4/entering/vocation_sheets.json` | `vocation_initequip_templ` and `vocation_templ`, decoded from D3 with a standard-library reader |
| L4 | `re/m4/world-survey/` | The design-time survey (entering, sync and movement, chat, the start and outfit): the leads this reading checked |
| M4E1 | `re/runs/out/m4-e1/`, with screenshots after 295 s in `re/m4/t11/m4-e1/` | Task 11's first world run (§13): `server.log` (trace), `gujianol.log`, `wine.log`, screenshots and the watchdog's log |
| M4E2–M4E4 | `re/runs/out/m4-e2/` to `m4-e4/`, with screenshots in `re/m4/t11/m4-e2/` to `m4-e4/` | The same for runs m4-e2 to m4-e4 |
| M4R | `re/m4/t11/runs.md` | The runs' predictions, written before each run, and their outcomes |

### REA Evidence IDs

Read through REA 6.3.0 with Ghidra 12.1.4 on the pinned exe, in two sessions on 2026-10-14, the second for eleven IDs the first had missed; each reading shows the function in the role the finding gives it. The session's first call, on FUN_140222E00, returned the ID [`../opcodes.md`](../opcodes.md) gives for it. An ID covers its call exactly as sent, so anyone can check it by repeating the call ([`../../re/README.md`](../../re/README.md#how-to-cite)). The IDs for FUN_140221AC0, FUN_140246E40, FUN_140246DF0 and FUN_1400B5670 are in [`../transport.md`](../transport.md), and FUN_1400D5850's in [`../payloads.md`](../payloads.md).

| Function or address | Cited in | Call | Evidence ID |
|---|---|---|---|
| FUN_140222E00 | X1; 4.1, 6.1, 7.3 | `procedure_pseudo_code {procedure: "0x140222e00"}` | `ev_4656519e08dab96fa5ad38e228a578375ef1b30907fb075530e4d9e15b4d3d20` |
| FUN_140221BB0 | X2; 3.1 | `procedure_pseudo_code {procedure: "0x140221bb0"}` | `ev_cecccd177df87001878f48ca00d2f3afac79247d8791f6627bb9c732374b329c` |
| FUN_140246E90 | X2; §3 | `procedure_pseudo_code {procedure: "0x140246e90"}` | `ev_6f141088ac237738a4cefbc084410bb1ba6f7406d0c8a7fcc00c6f3150cf5690` |
| FUN_1402481D0 | X2; §3 | `procedure_pseudo_code {procedure: "0x1402481d0"}` | `ev_b0eea52e8b77836220a2b4db4f0aae98cad374e132cd2626522739d4ce8bf7a5` |
| FUN_141156EA0 | X2; §3 | `procedure_pseudo_code {procedure: "0x141156ea0"}` | `ev_34c9484b24a9e2a646a11c7ea43be750d7facc681a8ec0729c6ff26c5e82e36f` |
| FUN_141156CC0 | X2; §3 | `procedure_pseudo_code {procedure: "0x141156cc0"}` | `ev_0b24736d7cb09b6b37c789bf2b54ca7faf69d722bc2680e64d276908eb9ca13d` |
| FUN_140A9D6A0 | X3; 7.2 | `procedure_pseudo_code {procedure: "0x140a9d6a0"}` | `ev_a4cb07077fee1f70cd667c3fb87bca67cfae3a05c3288a33d35edd9516e23da6` |
| FUN_140A9D650 | X3; 7.2 | `procedure_pseudo_code {procedure: "0x140a9d650"}` | `ev_ebadafa337fa049acb9f1be23b40b3107f6def7a423c100743ccdf6f9c50ce37` |
| FUN_141156C20 | X3; 7.2 | `procedure_pseudo_code {procedure: "0x141156c20"}` | `ev_8118a9cb7936b7e6ac537ab837c00bd640d359a882181cfb6f12e778eba54580` |
| FUN_1401AF730 | X4; 4.3 | `procedure_pseudo_code {procedure: "0x1401af730"}` | `ev_2f0497d57d526f9b5cf20127fa9d20eb3645cb70d1d63a6fc236a8858d207373` |
| 0x1401AFCDC | X4; 4.3 | `inspect_native_instruction {address: "0x1401afcdc"}` | `ev_b0b4c4c10512320d8ab8fbc5349e3396575d04d257f81cd825b387e5b5eb93f5` |
| FUN_1400E2D60 | X4; 4.3 | `procedure_pseudo_code {procedure: "0x1400e2d60"}` | `ev_67c7d5abc78e75378634ab16f7c732e22d2cd6a5c74ffbcd976ead04d7435088` |
| FUN_1401ACE70 | X4; 4.3 | `procedure_pseudo_code {procedure: "0x1401ace70"}` | `ev_d58c0b136dd0445d63f9525901ea87abc7c17719a1293540febd8a07d686508e` |
| FUN_1401ACD10 | X4; 4.4, 4.5, 4.7 | `procedure_pseudo_code {procedure: "0x1401acd10"}` | `ev_a82e1dd19b4ee9ca02724433a95092af9118a067430e1ab17b2888edce13dc1e` |
| FUN_14017BA60 | X4; 4.4, 4.6 | `procedure_pseudo_code {procedure: "0x14017ba60"}` | `ev_da91bba28ac61238a756ace6376f3da3af3d2085abb8727c24e194086817605f` |
| 0x14017BD64 | X4; 4.4 | `read_bytes {address: "0x14017bd64", length: 60}` | `ev_7d1bb9e9f2395d5ccb6945ed2e8f79e0a89d33c4eaea21ff38f2819c5d4eaf7e` |
| FUN_14017BED0 | X5; 4.10 | `procedure_pseudo_code {procedure: "0x14017bed0"}` | `ev_857afb28823f844c3a980a74d96db96f9da2bbb574562e8b0c3dfbcc23e53d5a` |
| FUN_14017C990 | X5; 4.9 | `procedure_pseudo_code {procedure: "0x14017c990"}` | `ev_59d1cd55f27f3c1b5e2af9a5a0ffea47a339e67a98abbc2197004fa07b3580ba` |
| FUN_14017D830 | X5; 4.10 | `procedure_pseudo_code {procedure: "0x14017d830"}` | `ev_49e0aa1cd767c4bc6fbfb7ef315408b1d0efa7f0c0d08d76c6261678bbfaec95` |
| FUN_14017CBA0 | X5; 4.10 | `procedure_pseudo_code {procedure: "0x14017cba0"}` | `ev_7804c494747392e2fcbf879c8e54fd9c6d506edd16df7a775e94686933d5958a` |
| FUN_14017BE80 | X5; 4.9 | `procedure_pseudo_code {procedure: "0x14017be80"}` | `ev_f6ccb8709532aa406b972bce1c7b7eef5edebeb8a9b3882e5b57ef839327c253` |
| FUN_14017BDF0 | X5; 4.10 | `procedure_pseudo_code {procedure: "0x14017bdf0"}` | `ev_5ade9c847801de2c9e61c90fa3868c728bfff9d09af44190d2eff373a70150a3` |
| FUN_14017B4C0 | X5; 4.10 | `procedure_pseudo_code {procedure: "0x14017b4c0"}` | `ev_09ef04e4f461fa9066224b5f042e048a81cef4e5c39f658417541a9b4e49c2a2` |
| FUN_14017DB20 | X5; 4.10 | `procedure_pseudo_code {procedure: "0x14017db20"}` | `ev_b930c6f3c46d286809dea7cbfd6a794ed972812f6b4f391f9e9fa56a5af8f17e` |
| FUN_14017DD30 | X5; 4.9 | `procedure_pseudo_code {procedure: "0x14017dd30"}` | `ev_e35e2bacc686421f278bd9f59bcf274ae7aca08ffbe72946be113e505470b15c` |
| FUN_14017DAD0 | X5; 4.9 | `procedure_pseudo_code {procedure: "0x14017dad0"}` | `ev_124abe3a4194620667d9b9d211f78d99d7b3d9239a85533a8dafffb4e3cdd82d` |
| FUN_1401B1E40 | X5; 4.10 | `procedure_pseudo_code {procedure: "0x1401b1e40"}` | `ev_c3b000e34eda7b160c7770b6fdffbaf7d67a1178f2cff490b9082fda90ba36cc` |
| FUN_14017DA60 | X5; 4.10, 4.12 | `procedure_pseudo_code {procedure: "0x14017da60"}` | `ev_4f81d1c67542b1598fdd2a9569a55e764269bd3498be7734edc3a3fc8372177d` |
| FUN_1401A9F30 | X5; 4.9 | `procedure_pseudo_code {procedure: "0x1401a9f30"}` | `ev_946fdf9d8a6886f45c043b70e5bb399527a1c703c7d11345fa1b43f752bff0f1` |
| FUN_1401B0470 | X5; 4.10 | `procedure_pseudo_code {procedure: "0x1401b0470"}` | `ev_1a6c09c506ec11a323a36be75b28339ff66bd793fb4aa6fa57e3e54072d91653` |
| FUN_1401B0910 | X5; 4.10 | `procedure_pseudo_code {procedure: "0x1401b0910"}` | `ev_4e2bb20b48022dfb3e88d78386d7cd7345d35b8fc02fe990a3a36fe22a4084b3` |
| FUN_1401AA020 | X5; 4.9 | `procedure_pseudo_code {procedure: "0x1401aa020"}` | `ev_fb287e43c0ba223bf8c064700e7114fc19893b840447291a816f2a050f0b1448` |
| FUN_14017BDB0 | X5; 4.9 | `procedure_pseudo_code {procedure: "0x14017bdb0"}` | `ev_09fbd8129436f80ad4390f8e05386b5db61506976839b56c1dc5ac04886a0686` |
| FUN_140097CA0 | X5; 4.9 | `procedure_pseudo_code {procedure: "0x140097ca0"}` | `ev_4b863a5bafe5e36e4214f3c60c5834aa594b14d86673516c5797b40cbb0a672c` |
| FUN_1401AC960 | X6; 4.8 | `procedure_pseudo_code {procedure: "0x1401ac960"}` | `ev_e7418dd30ca12985c251edb21fc7a23b70b6379ff6da9ad3572b28c0a70213a5` |
| 0x143105350 | X6; 4.8 | `read_bytes {address: "0x143105350", length: 48}` | `ev_b9e129b48185ec806fd6777420699d6945ad7b3969e63c97f722b495b2a75961` |
| FUN_140227530 | X7; 2.8 | `procedure_pseudo_code {procedure: "0x140227530"}` | `ev_665e1b81d3f0f5a8fc92874290e97a492ea1bbe7da793bbeea629d4fda2ededd` |
| FUN_1402463C0 | X7; 2.8 | `procedure_pseudo_code {procedure: "0x1402463c0"}` | `ev_1cbb824804c5661cddfca13766875830d58b5cdfd239ebdacebeeafdf7a43dcd` |
| FUN_1400E87E0 | X7; 2.8 | `procedure_pseudo_code {procedure: "0x1400e87e0"}` | `ev_346e7fef3f524bc043f398d4031e0ac678e1031e4051ca3fa8404bb6f9a06015` |
| FUN_1400E8790 | X7; 2.8 | `procedure_pseudo_code {procedure: "0x1400e8790"}` | `ev_781a0a1ed293a7961d6b69a8ddf5116b4d790115f0d6d46ac9ae2617f1421a62` |
| FUN_1400E8140 | X7; 2.8, 4.12 | `procedure_pseudo_code {procedure: "0x1400e8140"}` | `ev_6814bdb87f42bf7d3e6d71e9ad6c33490d8fae722a557df97d976c9c79e793dd` |
| FUN_14020CE90 | X8; 6.2 | `procedure_pseudo_code {procedure: "0x14020ce90"}` | `ev_3c7122d8e4e4e5f8f1e17bea4477d49d084c5e7143bd66f3bccc24e3882c50df` |
| FUN_14020BA70 | X8; 6.3 | `procedure_pseudo_code {procedure: "0x14020ba70"}` | `ev_f565044b26774ddf440f53f6347a55ac802a338567b7d2e93dca71df6e4858c7` |
| FUN_140211630 | X8; 6.2 | `procedure_pseudo_code {procedure: "0x140211630"}` | `ev_ae2ac0e76b0683dc8570f76b3e85f15eef94d08dce44da70b3601f8fcbc55bcc` |
| FUN_1402114E0 | X8; 6.2 | `procedure_pseudo_code {procedure: "0x1402114e0"}` | `ev_3c45bd025ad07fbfe9122fac61dbba3d47d6d69e2d1abb0cdae0fbaf328be8bf` |
| FUN_1401AD3A0 | X8; 6.2, 8.1 | `procedure_pseudo_code {procedure: "0x1401ad3a0"}` | `ev_04d840248fe76d0ec655981ee52c6ffcc24c948ed51228d58e6310f039dbf27b` |
| 0x14020DE10 | X9; 6.4 | `read_bytes {address: "0x14020de10", length: 17}` | `ev_8ab9766c4294b6caf732a8d86ecca94b3e8398063e54d906b3aabca1ccb91ab9` |
| 0x14020AAC0 | X9; 6.4 | `inspect_native_instruction {address: "0x14020aac0"}` | `ev_5c0455d26516e40297b485b66213393386c7f9b471b61a161cc1c4862d13a4db` |
| 0x142F04890 | X9; 6.4 | `read_bytes {address: "0x142f04890", length: 4}` | `ev_95b2cc515273960d7b82de561b2d4ab5210e23347c601ad0c346934bb5c0ee9f` |
| 0x142F046B8 | X9; 6.4 | `read_bytes {address: "0x142f046b8", length: 4}` | `ev_6aa21e6065a6b105629cbb0a1cb3031596646b97df919133f2ab068a0afefa24` |
| 0x140210A30 | X9; 8.1 | `read_bytes {address: "0x140210a30", length: 8}` | `ev_c9e0adb31ba1e1f163702debd370f47525902185d7eb6c123b1c51d5f3023c57` |
| 0x141189F00 | X9; 8.1 | `read_bytes {address: "0x141189f00", length: 2}` | `ev_62981121f736dbc4781d3b252b8c88f9b1cd5888eba8b697c44ff3537ea885c7` |
| FUN_1401AF2F0 | X10; 4.2, 8.1 | `procedure_pseudo_code {procedure: "0x1401af2f0"}` | `ev_b71792ca61faa1b2784aaab20d06b1a9db9898b6f45fd6a32a9c6f1de580a6cf` |
| FUN_140222DE0 | X10; 7.3 | `procedure_pseudo_code {procedure: "0x140222de0"}` | `ev_3942b189e3a5091a06c50210797945a849c3e9f63fd186cc7299caffeb724c9d` |
| the exe's strings | X11; 9.1 | `search_strings {pattern: "equips_id", mode: "literal"}` | `ev_b83f8a725bb3c9d6fe73c0a77111a20a8521532dd4e2a43e2d1d1133f8a3631e` |
