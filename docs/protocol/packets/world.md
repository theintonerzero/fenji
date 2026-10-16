# World: entering, seeing each other and moving

After the select screen's 10, the server answers with the script message 772. The client reloads its game modules, sets the player's actor id and loads the scene's map behind its loading screen, sending a time sync once a second from then on. When the map and the game's add-ons have loaded it reports the native type 3. Only then does the server send the player's own actor (type 7, with a property blob, which carries its move speed) and type 8, after which the loading screen closes and the client's in-world script messages flow. This document gives the evidence behind the values that `fenji_proto::world` uses for that, the format of the property blob that types 7 and 10 carry, the time syncs, and the creation outfit.

Other players then reach a client in three steps (§14): the server announces each one in view (type 6), the client pulls them one at a time, nearest first (type 5), and the server sends each one whole (types 7 and 8); type 9 takes them away. §15 reads what lets two clients run on one machine, for the two-client runs.

Players move as §16 reads: the client sends a direction command, type 8, when its player starts, stops or changes course; the server relays each one, as type 13, to the clients that show the player, and puts a player back where it has it with type 20.

**Method.** This reading comes from the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`). It used three sources, all read locally and read-only:
- the client's own scripts, disassembled and decompiled by `luatool` ([`../../re/m1-script-format.md`](../../re/m1-script-format.md));
- the client's own caches: the protocol table, `prop_tables`, two sheets of `all_tables` and `locale_en`, read with a standard-library probe of [`../payloads.md`](../payloads.md)'s format;
- the exe, in the shared Ghidra project and then through REA in one session (2026-10-14), after the evidence norm ([`../../re/README.md`](../../re/README.md#the-norm)); the Evidence IDs are in [REA Evidence IDs](#rea-evidence-ids). Where Ghidra's analysis made no function (0x14020DE10, 0x140210A30), the bytes were read through REA and disassembled with `objdump`. The exe's export table and its built-in property table were read with a standard-library PE reader.

§14 and §15 come from a second reading (Task 12, 2026-10-14; its notes are L5) by the same method: the scripts, the exe in the shared Ghidra project and then through REA in one session, and in another for 15.1's and X18's references (their IDs are in the same table), the exe's import table read with a standard-library PE reader, and Task 11's server logs (§13), read for what a lone client sent. Task 13's two-client runs (§13) then checked them against the real client, and 15.6–15.8 come from those runs.

§16 comes from a third reading (Task 14, 2026-10-14; its notes are L6) by the same method: the scripts, the exe in the shared Ghidra project and then through REA in two sessions (their IDs are in the same table), with `objdump` for the bytes at 0x140170C90 and 0x140174400, where Ghidra's analysis made no function, and Task 11's server logs (§13), which give each message's type and length only.

Task 15's two-client runs (§16's Runs, 2026-10-15) then checked §16 against the real client: what they showed is tagged **Observed** (runs) in §16, and they changed one value, the time bound of 16.15. Task 19 measured the world's tick and its shutdown with twenty protocol bots and no client (§13).

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
| 1.5 | **Back to the select screen.** `SCENE.RequestLogout` (`0.62`) sends type 4 with an empty body, after turning sends off. 330 `PROTOS2C_ACTOR_LOGOUT` takes the client back: its handler, with no arguments, turns sends back on and fires `SCENE.EVENT_LOGOUT`. On it, UIMain's `OnActorLogout` (unless the player was kicked) calls `ReturnSelectActor`, which leaves the scene, loads the login add-ons if they're gone, and shows the select screen through ActorSelect's `ShowSelectActor`. That screen shows the list it had, or asks for it again with `NET.QueryActors` when it has none. All of it is on the same connection. Fenji answers type 4 with 330 and gives the connection back to the lobby (#282). | **Observed** (registration; the 330 handler's, UIMain's and ActorSelect's bytecode), **Inferred** (that 330 answers 4: no client run yet) | S7 `0.62`; S2 `0.27`; S1 `0` [120]–[123]; S6 (`UIMain.luac`: `OnActorLogout`, `ReturnSelectActor`); S10 (`ActorSelect.luac`: `ReturnSelectActor`, `ShowSelectActor`) |

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
| 5.5 | **Chat's tables needn't be sent.** Unset, 389 (`prop_sys_chatbox_setting`) and 396 (`prop_sys_emoji_infos`) read as `{}` (4.11). The chat settings then fall back to their defaults, which the client sends with 422 at every read for as long as 389 is unset ([`chat.md`](chat.md) 4.2), and no emoji counts as unlocked (chat.md 4.3). Nothing fails. Fenji may send them as pickled tables later. | **Inferred**; the 422s **Observed** (chat.md 4.2) | 4.11; [`chat.md`](chat.md) 4.2, 4.3; L4 (chat survey §4, §10) |
| 5.6 | **Health must be sent.** Left at their defaults (304 `prop_num_result_hp_max` 1, 300 and 1458 0), run m4-e1's HUD showed "HP 0" and the whole scene grey. The player model's setup registers `OnHPChanged` for 1458 and 304 and calls it once (S17 `0.4` [6]–[24]). It sets the model's health target to 1458 ÷ 304 when 1458 is 0, and to 1 otherwise (`0.20` [9]–[15]). While that target is at most 0.6, the model's update turns on the `PostDead` post-effect with a saturation of target ÷ 0.6 (`0.21` [173]–[266]), so a current health of 0 draws the scene without colour. The scripts read 300, 1458 and 304 through `GetHPRat`, `GetHP` and `GetHPMax` (S18 `0.47`, `0.48`, `0.50`). Death itself is property 15 `prop_base_dead`, which defaults to 0. So Fenji sends full health: 300 = 1.0, and 304 and 1458 = `FULL_HEALTH` (1000; M4 has no combat, and the value is Fenji's choice). | **Observed** (bytecode; run m4-e1), **Inferred** (that 1458 caused the grey) | S17; S18; D2; M4E1 |

## 6. Type 8, and the speed

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 6.1 | **Type 8's body is the actor id,** a `u64` LE (FUN_140222E00's case 8, which case 10 falls into after pushing its blob). The handler calls `actor_system_enter_finish(id)` (1.2). | **Inferred**; offset **Observed** | X1; S2 `0.10`; S3 `0.45` |
| 6.2 | **Type 23 `SYNC_PROP_FORCE`** (FUN_14020CE90): a `u64` actor id, a `u16` property index and an `i16` value, 12 bytes. Index 1325 sets `prop_move_speed` and 1330 `prop_move_gravity` (FUN_140211630, FUN_1402114E0, each through the property's own setter, the one the blob uses); other indexes are ignored. When the actor is the player's own (the move state's flag at +0x112, which 16.3's sender also requires), it also clears DAT_1431F37A8, the time of the last direction command, so the next one goes out at once (16.6). The actor must exist (FUN_1401AD3A0 finds it by its integer id), so 23 comes after 7. | **Inferred**; 0x52D and 0x532 **Observed** | X8; X22 |
| 6.3 | **Type 21 `MOVE_BASE_PROP`** (FUN_14020BA70): a `u64` actor id, a `u64`, three `i32`, an `i16`, then the speed and the gravity as `i16`, 34 bytes. It also writes the middle fields into the actor's move object (meaning **Unknown**), and a short body leaves stale values in them. So Fenji uses 23. | **Inferred** | X8 |
| 6.4 | **The rate.** `move_get_move_speed_by_rat` (0x14020DE10, bound at 0x14020AAC0; Ghidra's analysis made no function there) multiplies the rate by 500.0 and by 0.01 (the `f32` constants at 0x142F04890 and 0x142F046B8): rate 100 is 500 units, and the step code makes that a second (16.9). The scripts' logic speed is `prop_move_speed` less an offset for backward moves, and a rate below 60 picks walking (L4; 16.7, 16.14). | **Observed** (bytes; the step's constants, 16.9) | X9; X24; L4 |
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

The item ids are documented interoperability data, by the owner's decision of 2026-10-13 (the M4 world spec §3.2, #180): M4's server reads no game data, so Fenji ships these ids. Since #284 the open decoder `fenji-oodle` reads the client's compressed tables, so a server could take them from the host's own copy; that loader is M5's (#180).

## 10. Closing a player's connection

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 10.1 | **What the player sees.** A type-1 message closes the connection, its body becoming the close reason ([`../transport.md`](../transport.md) 7.4). In the world, UIMain's net-status timer (`0.3`) then shows the text `NET.FetchNetErrorByErr(reason)` finds for state 5, or a generic text about a server or connection failure when it finds none; the reason `client-exit` makes the client quit instead. A connection the server just drops ends in state 4 once the client's next send fails ([`../transport.md`](../transport.md) 1.4–1.5), and for state 4 the timer shows a fixed text about server maintenance. | **Observed** (bytecode), **Inferred** (the states) | S6 `0.3`; [`lobby.md`](lobby.md) §9; D4 |
| 10.2 | **The keys the link can use** ([`lobby.md`](lobby.md) 9.3): `error duplicate-login`, M3's choice for a login that a newer one replaced; `error server-not-running`, whose text says the server is unavailable for now; `error kick-by-gm`, whose text says suspicious activity was found on the account; and `error kick-by-user`, whose text says the player is offline and may log in again. | **Observed** (keys; D4's English texts) | S16; D4 |
| 10.3 | **Fenji's choices:** a replaced login gets `error duplicate-login`; a server stopping gets `error server-not-running`; an admin's `!kick` gets `error kick-by-user`, which accuses the player of nothing, unlike `error kick-by-gm`; a client that stops reading or floods gets no reason, so it sees 10.1's maintenance text; a client quitting (392) gets `client-exit`, the client's own reason, on which it quits (#382). | the choices are Fenji's | 10.1, 10.2 |

## 11. What Fenji sends

| The client sends | Fenji answers | Values |
|---|---|---|
| 10, a pid of the account's | 772 | `(772, pid, {scene_uid = scene, scene_tid = scene, scene_lv = 0, scene_type = 0}, {x, y, z, yaw}, 4706187382722034291)`, the position as floats |
| type 2 | type 2 | the client's `u32`, and the server's Unix time in ms (§7) |
| type 3 | type 7, type 8 | the header `(pid, 1, 255, vocation × 10000)` and §5's blob, with the speed and full health; then `pid` |
| type 3 of any length but 21 bytes | closes the connection | without a reason; only the length is checked (3.2) |
| the cached type-0 script messages | nothing | none needs an answer in slice 1 |
| 392 (0xFF) | closes the connection with the reason `client-exit` | the client quits on it (10.1). Closed without it, the client took the close for a lost connection: its login screen, then the login error (#382) |
| type 4 | 330 (no arguments), and the connection goes back to the lobby | the client returns to its select screen on the same connection (1.5; #282) |

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

Only a run of the real client can settle these, each run's prediction written before it starts. Task 11's runs (`scripts/spike/run-client.sh` with `--server`) answered 1–6, and Task 13's two-client runs 7 and 9–11 (§13), apart from question 9's health bar, which the client doesn't show for another player; 8, 12 and 13 are still open.
1. **The sequence works:** after 10, the loading screen for scene 5 closes and the character stands at 772's position; the server's log shows type-2 syncs from 772 on, then type 3, then the client's flushed script messages after type 8. Task 11's first world run.
2. **The character walks** with `prop_move_speed` 100, at about 500 units a second, and whether the blob or type 23 alone would do (6.5). The same run, walking for a timed stretch.
3. **The look:** the face, the voice and the outfit as at creation, with the head hidden (5.3). The run's screenshots.
4. **The HUD's health bar** with 300, 304 and 1458 at their defaults (5.6). The screenshots.
5. **Units:** that 772's x, y and z are centimetres with z up and the yaw degrees, and whether the client puts the character on the ground when the start's z is above it (2.8). The run's position reports.
6. **A pid above 2^32** (8.3). A run with a character whose pid is seeded above 2^32.
7. **Two clients in one Wine prefix** (world spec open question 7). The exe lets a second copy run beside the first (15.1), so both share one prefix, each from its own copy of the client (15.5). Task 13's first two-client run: both reach the world, and the watchdog sees no `winedevice.exe` start with the second.
8. **The type-2 reply's effect:** that dates the client shows follow the server's time. The run, with the client's clock set away from the host's.
9. **Seeing another player** (14.7–14.10): each client shows the other where it stands, in its creation outfit and face, with its name and a full health bar, after 6, 5, 7 and 8, and neither client logs an error for it. Task 13's first two-client run.
10. **Leaving** (14.11): when one client's connection ends (the second client's run ends first), the other's client takes that player away on 9, and its log shows no error. The same run. Walking out of view waits for the movement slice (Task 15), since until then the world doesn't learn where a player walks, and an admin's `!tp`, which makes a 9 and a new 6, for the chat slice (Task 16). **The `!tp` and `!kick` cases answered** (Task 16's t16-c1 and t16-c3, [`chat.md`](chat.md) §7): on a near `!tp` Mai's client took Lin away on her 9 and showed her beside him on the next tick's 6, 7 and 8; on `!kick Mai` Lin's client took Mai away on his 9; neither client logged an error.
11. **The filter** (14.12–14.13): the client's type 5 with 32 for the other player arrives within seconds of its 7, and Fenji ignoring it changes nothing either player sees. The same run.
12. **Back to character select** (1.5; #282; the follow-ups plan's Task 8): after type 4, 330 shows the select screen with the account's characters, on the same connection, with no error. A second 10 then gives 772 at the place the character stood, and the character enters as it did the first time. The prediction for that run, which waits for the owner (Steam must not run beside it).
13. **Quitting** (10.1; #382): after Quit Game in the world, the client gets the close with `client-exit` and quits at once, with no login error. The same run.

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
- §12's question 8 (the reply's effect on the dates the client shows): not run. Question 7 is Task 13's, below.
- In m4-e4 alone, `g` also opened the World Map, and the client sent 85 `PROTOC2S_ITEM_COLLECTION_USE`; the same key in m4-e2 and m4-e3 only started the run.

Task 13's runs (2026-10-14) ran two clients at once in one sandbox, one Wine prefix and one `fenji-server`, with `run-client.sh run --clients 2` ([`../../re/README.md`](../../re/README.md)). Each client ran from its own reflink copy, with its own account (`tester`, `second`), Proton log and nested display (`:99`, `:97`). Client 2 started 30 to 240 s after client 1, and from m4-s3 on in a Wine desktop of its own (15.7). The server, the launcher and the client's arguments were as in Task 11's runs, on one database (`re/runs/server-data-s`) that held both accounts. m4-s0b and m4-s0c made the characters there: Lin (pid 1, Fox Mage) and Mai (pid 2, Spellsword). Each later run used a fresh copy of that database in which Mai stands 2 m ahead of the start, facing back toward it (scene 5, (2559, −71787, 64854), yaw 165; 195 in m4-s5). Lin has no place, so she enters at the start. Two rehearsals without a client came first (15.5, 15.6). Predictions were written before each run (M4R13).

| Run | What the run did | What happened | Watchdog | Evidence |
|---|---|---|---|---|
| rehearsal | Wine's `notepad.exe`, then 30 s later a second `proton run` of `cmd.exe` in the same prefix; no client | one wineserver; no wineboot, services.exe or `winedevice.exe` came with the second; services.exe answered session 2's calls in log 1; the second `proton run` never returned (15.6) | `scanlog`: no sign | M4RH |
| m4-s0 | both clients make characters; client 2 30 s later | both ran in the prefix and reached the create screen; input crossed between the clients on their one Wine desktop, so no character was made; client 2's `gujianol.exe` outlived its timeout, because the runner looked for it by a process name that its main thread's renaming had changed | no sign | M4S0 |
| m4-s0b, m4-s0c | one client each: `tester` makes Lin, then `second` makes Mai | Lin (vocation 7, skeleton 3) and Mai (vocation 2, skeleton 1) made | no sign | M4S0B, M4S0C |
| m4-s1 | Lin enters alone; client 2 starts 240 s later, with clicks only | Lin stood at the start; client 2 took none of its clicks and stayed on its title, and was ended by its own pid 10 s after its timeout | no sign | M4S1 |
| m4-s2 | client 1 with keys only, client 2 with clicks only, 30 s apart | Lin entered; client 2 took none of its clicks | no sign | M4S2 |
| desktop rehearsal | `notepad.exe`, then 30 s later `explorer.exe /desktop=fenji-client2,1280x720 notepad.exe` on `:97`; no client | one wineserver and the same two `winedevice.exe`; the second start brought only `explorer.exe`, notepad and `tabtip.exe` | `scanlog`: no sign | M4RD |
| m4-s3 | client 2 in its own desktop; keys only | client 2 took its keys and Mai stood alone; client 1 took none, since the pointer rested outside its window (15.8) | no sign | M4S3 |
| m4-s4 | Lin enters first (amended during the run: a click, then Return), then Mai | **each client pulled the other and got its 7 and 8; client 1 showed Mai**; client 2 showed Lin's board, with Lin behind Mai's body; the 9 took Mai from Lin's view | no sign | M4S4 |
| m4-s5 | as m4-s4, with Mai turned 30° aside and the click planned | **each client showed the other standing, in its creation outfit, under its name board**; the 9 took Mai from Lin's view | no sign | M4S5 |

**Observed (Task 13):**
- **Two clients in one Wine prefix** (§12 question 7). In every run both copies ran at once (`GJMut2` let the second start, 15.1), each logging to its own copy's folder, under one wineserver; from m4-s4 on both reached the world, and each started its own web view. The watchdog followed both logs and both clients. It saw no `winedevice.exe` start after client 1, and no new `.sys` file or service key, in any run (15.5).
- **The sequence** (§14), the same in m4-s4 and m4-s5. On the second player's type 3, in one tick, that player's own 7 and 8 went out, and a 6 each way, each at the other's place. Each client sent one type 5 with the other's pid and 0, and got the other's 7, then its 8, within 31 ms. The client already in the world sent it 91 ms (m4-s5) and 270 ms (m4-s4) after its 6; the one that had just loaded, 1.35 s and 1.47 s after. Then each sent one type 5 with 32, 2.8–3.4 s after its 7, which Fenji dropped. No other type 5 came, and neither client logged anything about seeing.
- **The bodies.** Type 5 was 18 bytes: the pid, the value, a byte 1, a zero `u32` and the machine byte (masked), as 14.4 reads them. Client 1's pull and its 32 in m4-s4 are fixtures, with byte 17 zero (`crates/fenji-server/tests/fixtures/m4-s4-*.hex`).
- **Seeing another player** (question 9). Each client made the other's actor where the world placed it: Mai 2 m up the path in Lin's view, and in m4-s5 Lin beside Mai in his. Each stood in its creation outfit and face: Lin with the Fox Mage's ears, tail and ring, Mai in his robe with his sword. Over each was a green name board with its name, and its idle pose changed between shots. The board came at once, the model later, as lavapipe loaded it: not yet 12 s after the 7 in m4-s4, there 28 s after it in m4-s5. **No health bar over the other player**: the board shows the name only (14.9).
- **Leaving** (question 10). When client 2's connection ended, client 1 got a 9 for pid 2 alone, 5 ms (m4-s4) and 30 ms (m4-s5) later. Mai vanished from Lin's view, and client 1 logged nothing.
- **The filter** (question 11): one 32 for each actor, about 3 s after its 7, and nothing either player saw changed.
- **No type 5 can share a read with the 10** (Task 11's review). In every world run, the client sent nothing between its 10 and the 772. Its next message was a type-2 sync 1.6 s after the 772, and a 5 needs a 6, which comes only after the client's type 3.
- **The clients' own errors.** Each client logged the three listener errors above, and Mai's the cooldown sweep's `SetRotation`, as in Task 11's runs. Lin's client logged a new one in every Task 13 run where she entered (m4-s1, m4-s2, m4-s4 and m4-s5): `bad argument #1 to 'SetY'` in the `Acquirement` addon's `OnPreRenderUpdate`, every 10 s from 2–3 min after entering. It came while no other player was in the world too (m4-s1, m4-s2), and it stopped nothing.

**Inferred (Task 13):**
- The client that had just loaded pulled later because the pull's timer runs from the client's frame loop (14.3), and the first frames after a load are slow on lavapipe.

**Unknown (Task 13):**
- Whether the name board shows another player's health at all, in combat or when that player is selected (M5).
- Why Lin's client logs the `Acquirement` error in Task 13's runs and never did in Task 11's.
- Whether two `umu-run` sessions share one wineserver, which the bare runner's two-client mode rests on (15.5).

Task 19's measurement (2026-10-15) ran no client. The load harness, `crates/fenji-server/tests/load.rs` (Task 17; the plan's Amendment D), runs a server and twenty protocol bots in one process. Each bot signs in, makes a character and enters at `[world]`'s start, so all twenty stand in one cell, and each pulls the other 19. Then for 60 s each moves about 40 units every 100 ms and says a nearby line every 5 s, while a reader drains its socket. The harness prints the world's tick report (`RunningServer::ticks()`), whose percentiles cover the last minute of ticks, which is the play. It then times the shutdown with all twenty still in, which saves each one and closes the database. The targets are the design spec's §6.7: tick work at p99 of at most 10 ms with 20 players in the starting zone, against the 50 ms budget of a tick at 20 Hz, and all 20 saved within 2 s of a shutdown. It ran three times as `FENJI_LOAD_ASSERT=1 cargo test --release -p fenji-server --test load -- --ignored --nocapture`, with `TMPDIR` set to a folder made on the data disk (`target/load-tmp`) rather than `/tmp`, a tmpfs, so that the database was on the disk. The Linux game box was otherwise idle:
- an AMD Ryzen 9 9950X3D, with 16 cores and 32 threads, and 62 GiB of memory;
- Fedora 44, with Linux 7.2.9;
- the database on a Samsung 9100 PRO NVMe SSD (2 TB), in btrfs with zstd compression;
- Rust 1.97.1's release profile.

| Run | Tick work p50 | p99 | Max | Ticks since the start (over budget) | Shutdown with 20 in | Sent |
|---|---|---|---|---|---|---|
| m4-l1 | 80.1 µs | 147.7 µs | 2.28 ms | 1,261 (0) | 66.4 ms | 11,980 moves, 240 lines |
| m4-l2 | 81.9 µs | 119.9 µs | 181.6 µs | 1,263 (0) | 64.6 ms | 11,980 moves, 240 lines |
| m4-l3 | 82.0 µs | 117.0 µs | 129.0 µs | 1,261 (0) | 64.1 ms | 11,980 moves, 240 lines |

**Observed (Task 19):**
- **Both targets met, far inside.** The tick's p99 was 117 to 148 µs, under 1.5% of the 10 ms target, and no tick in the three runs went over its 50 ms budget; the slowest of the play took 2.28 ms (m4-l1). The shutdown took 64 to 66 ms, about 3% of the 2 s target.
- **The full load, in every run.** Each bot sent all 599 of its moves and all 12 of its lines, and got 599 13s and 12 378s from each other bot; the world refused no move, and each bot was saved where it last stepped. With `FENJI_LOAD_ASSERT=1` the harness asserted both targets, that no move was refused, and that each bot sent at least 95% of its moves and of its lines, rounded down (569 and 11).
- **The disk's share of the shutdown.** A fourth run, m4-l4, with the database on `/tmp`, shut down in 49.8 ms (p50 83.2 µs, p99 122.5 µs, max 360.3 µs), so the disk added about 15 ms.

**Unknown (Task 19):**
- The tick with the monsters around the players, which §6.7's target also counts and M5 brings.
- How a host smaller than this one does, such as a small VPS.

## 14. Seeing each other

Another player reaches a client in three steps, and the client chooses when: the server announces it, the client pulls it, and the server sends it whole.

| Step | Direction | Message | What happens | Evidence |
|---|---|---|---|---|
| 1 | S→C | type 6 `STC_PROTOCOL_SYNC_ID_ENTER`: the id and where it stands | the client queues the id; nothing is shown yet (14.2) | X1; S2 `0.8`; S19 `0.13`, `0.8` |
| 2 | C→S | type 5 `CTS_PROTOCOL_SYNC_FILTER_SET`: the id and 0 | every 250 ms the client takes one queued player id, nearest first, and pulls it (14.3) | S19 `0.10`, `0.11`; X13 |
| 3 | S→C | type 7 `STC_PROTOCOL_SYNC_ENTER` for that id | creates the actor, with its model, name board and move handle, at property 16's place (14.7–14.9) | S2 `0.9`; S3 `0.52`, `0.33`; X14 |
| 4 | S→C | type 8 `STC_PROTOCOL_SYNC_ENTER_FINISH` for that id | combat's enter hook and `SCENE.EVENT_ACTOR_ENTER_FINISH` (14.10) | S3 `0.45`; S22 |
| 5 | S→C | type 9 `STC_PROTOCOL_SYNC_LEAVE`: ids | an id still queued is dropped from the queue; any other actor is destroyed (14.11) | X1; S2 `0.11`; S19 `0.14`; S3 `0.46` |

Type 6's body:

| Offset | Bytes | Field | Pushed to the scripts as | Fenji sends | Tag |
|---|---|---|---|---|---|
| 0 | 8 | actor id, `u64` LE | an integer, all 64 bits | the pid | **Observed** (runs: each client pulled the pid its 6 named, M4S4, M4S5) |
| 8 | 4 | x, `i32` LE | an integer, sign-extended | x, rounded to the nearest integer | **Inferred**; offsets and widths **Observed** |
| 12 | 4 | y, `i32` LE | the same | y, the same | the same |
| 16 | 4 | z, `i32` LE | the same | z, the same | the same |
| 20 | 1 | the fast flag | a boolean, `true` for any byte but 0 | 0 | the same |
| 21 | 1 | the priority | an integer, 0 to 255 | 0 | the same |

Type 5's body, 18 bytes:

| Offset | Bytes | Content | Source | Fenji | Tag |
|---|---|---|---|---|---|
| 0 | 8 | the actor id, `u64` LE | the script's id, Lua stack slot 2, through `luaL_checkinteger` (FUN_140119940) | reads it | **Observed** (runs: the other player's pid, M4S4, M4S5) |
| 8 | 4 | the value: the integer's low 32 bits | the script's value, slot 3, the same way | reads it: 0 pulls (14.13) | **Observed** (runs: 0, then 32) |
| 12 | 1 | 1 | a constant | | **Observed** |
| 13 | 4 | 0 | FUN_140246EA0 writes 0 | | **Observed** |
| 17 | 1 | 1 when a query about the client's own process returns a non-zero value, else 0 | FUN_141157040 (14.4) | neither reads nor logs it | **Inferred** |

Type 9's body is a run of actor ids, each a `u64` LE, with no count: as many as the length holds, 8 bytes each.

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 14.1 | **Type 6's body.** FUN_140222E00's case 6 (0x140222F3E–0x140222FA9) reads the id (`MOV RDX, qword`), then x, y and z with `MOVSXD`, then two bytes with `MOVZX`: it pushes the first as a boolean (FUN_140116AA0, `true` for any byte but 0) and the second as an integer (FUN_140116840). That is 22 bytes; the length isn't checked. The handler (`0.8`) passes the six values to `actor_id_enter`. | **Inferred**; offsets and widths **Observed** | X1, X12; S2 `0.8` |
| 14.2 | **An announced id is queued, not shown.** `actor_id_enter` puts an id whose `id >> 48` is 0 in the player queue (`0.13` [1]–[2]); the flag picks the fast queue for NPC ids only ([4]). The enqueue (`0.8`) drops the id, and empties the player and NPC queues, while the client has no local player ([1]–[7]); for an id already queued it updates x, y and z only ([10]–[18]); otherwise it keeps the time, x, y, z and the priority ([19]–[45]). | **Observed** (bytecode) | S19 `0.13`, `0.8` |
| 14.3 | **The pull.** A timer, `player_id_enters`, every 250 ms and one id each time (`0` [69]–[75]), which the timer service sets again after each call (S21 `0.6` [17]–[18]), takes one id from the player queue (`0.10`): the highest priority first ([36], [64]), then the nearest to the local player, with every coordinate × 0.01 ([18]–[20], [38]–[49]), then the earliest queued ([56]–[60]). For it, the client sends type 5 with the id and 0 (`0.11` [24]–[27]). So a client pulls at most one player every 250 ms, nearest first. NPC ids have a queue of their own, and a fast queue that takes five each time. In the runs each client pulled the one player announced to it once, 91 ms to 1.47 s after its 6 (§13). | **Observed** (bytecode, runs), **Inferred** (the rate) | S19 `0`, `0.10`, `0.11`; S21; M4S4, M4S5 |
| 14.4 | **Type 5's body.** `send_sync_filter` (FUN_140221D30) posts type 5 with length 18 (0x12), laid out as the table above. Byte 17 comes from FUN_141157040, which picks one of four stubs by the Windows version, each a direct system call (`mov r10, rcx; mov eax, 0x16` to `0x19; syscall; ret`, at 0x14029990C), calls it with the current process and the class 7, and gives 1 when the call succeeds with a non-zero answer. That the class asks whether a debugger is attached is **Inferred**; the byte describes the player's machine, so Fenji neither reads nor logs it (as 3.2). The script's sender checks only for state 3 (S2 `0.29`), so type 5 goes out while type-0 sends are off (1.3). Two scripts send it: the pull (14.3) and the sync filter (14.12). | **Inferred**; constants, widths and the stubs' bytes **Observed** | X13; S2 `0.29`; S19 `0.11`; S20 `0.3` |
| 14.5 | **A lone client sends no type 5.** In Task 11's runs, and with Mai alone in the world in m4-s3, the client sent native types 2, 3 and 8 only in the world (§13): it was told of no actor, and the filter leaves out the local player (14.12). | **Observed** (runs) | M4E1–M4E4, M4S3 |
| 14.6 | **The server may send another player's 7 without a pull** (world spec open question 4): the type-7 handler creates a non-local actor whenever a local player exists, without looking at the queue (S2 `0.9`; `actor_id_on_actor_sync` keeps only a debug history, S19). But the pull spaces out the client's model loads, nearest first, so Fenji sends it only after the pull, once: the world answers a pull only for a pid it announced and hasn't sent. A 6 for an actor the client already shows would queue it again, and that pull's 7 would reset its properties (14.7), so the world announces a pid once while it stays in view. | **Observed** (bytecode), the choice is Fenji's | S2 `0.9`; S19 `0.8`; S3 `0.52` |
| 14.7 | **Creating another player.** `actor_system_create_actor` returns at once for an id that exists (S3 `0.52` [1]–[6]), and the type-7 handler then resets its properties from the new blob (`0.51`). Otherwise the actor is made as the player's own is (4.2): the header's type, difficulty and template, the blob, then the handles (`0.24`), and `SCENE.EVENT_ACTOR_ENTER` fires (`0.33` [152]–[156]). Only for the local id does `actor_system_local_player_enter` run. In the runs each client made the other's actor from its 7 (§13). | **Observed** (bytecode, runs) | S3 `0.52`, `0.51`, `0.33`, `0.24`; M4S4, M4S5 |
| 14.8 | **Where it stands is property 16.** Players get a native move handle (`0.20`, `0.21`). Its creation, `actor_move_create` (FUN_1401FF980, bound at 0x14020A3BC), starts it at property 16 (FUN_14018D230), as four integers, and does its local-player setup only when the id is the local one. The handles are made after the blob is unpacked (4.3), so 16 places another player as it places the player's own (5.2). Type 6's x, y and z only order the queue. In the runs each player stood at its place, which its 6 and its 7 both named (§13). | **Inferred**; the property offsets **Observed**: FUN_14018D230 and FUN_14018D2E0 read the class's entries at +0x1538 and +0x57F88, which are indexes 16 and 1098 at 0xB8 + index × 0x148 | X14; S3 `0.20`, `0.21`, `0.24` |
| 14.9 | **Another player's 7 is the player's own 7 for that character** (§5). Every property in that blob is of sync class `all`, but 1325 and 1458 (`""`); none is `self` (D2). The model reads the same properties for any player (S11 `0.26`). The name board is made on `SCENE.EVENT_ACTOR_ENTER` (S23 `0.17` [33]), and among the properties it watches are the name (1) and the health (1458, 304) (S23, `BillBoardActor.luac` `0` [89]–[109]). Health draws the grey scene only for the local player: `OnLocalActorEnter` (S17 `0.4`) registers `OnHPChanged`, and another player gets `OnActorEnter` (`0.2`), which registers an animation event only. In the runs, the other player stood in colour, in its creation outfit and face, and its board showed its name only, with no health bar (§13); when the board shows one is Unknown. The speed, 1325, is what its move handle moves it by (6.4). So V7 (§11) is also Lin's 7 as another client receives it. | **Observed** (runs: the look and the board); **Inferred**; the classes and the handlers **Observed** | D2; S11 `0.26`; S17 `0.2`, `0.4`; S23; M4S4, M4S5 |
| 14.10 | **Type 8 for another player** (S3 `0.45`) runs combat's enter hook, `recoil_on_player_enter` for a player (S22 `0.143` [24]), and fires `SCENE.EVENT_ACTOR_ENTER_FINISH`, whose one listener acts only on an actor carrying a teleport effect's custom value (S24 `0.16` [11], `0.14`). For a non-local id it leaves sending alone. Seeing a player waits for nothing in it; Fenji sends it after every 7, as the client's own flow ends each entry. | **Observed** (bytecode), **Inferred** (not needed) | S3 `0.45`; S22; S24 |
| 14.11 | **Type 9.** FUN_140222E00's case 9 (0x140223006–0x14022304A) pushes a table of `length >> 3` ids, keyed from 1 (FUN_140116EF0, FUN_140117630). For each id the handler (S2 `0.11`) calls `actor_id_leave` (S19 `0.14`), which drops the id from whichever queue holds it, and only when none did, `actor_system_destroy_actor` (S3 `0.46`), which returns quietly for an id it doesn't know (`0.34` [4]–[9]). For the local id it calls `actor_system_local_player_leave` ([41]–[47]): **a 9 must never name the player's own pid.** In the runs a 9 for the other player took it from view, and the client logged nothing (§13). | **Observed** (bytecode, offsets; the effect, runs) | X1, X12; S2 `0.11`; S19 `0.14`; S3 `0.46`, `0.34`; M4S4, M4S5 |
| 14.12 | **The sync filter** (world spec open question 8). Every 3 s (`actor_filter_update`, S20 `0.7`) the client works out a value for each other player and NPC (`0.6`): 0 for an actor it focuses, has selected, duels (property 188) or ranks in its top 10 threat; otherwise the level of the row that type 3 `TRANS_LIMIT_LV` picked, with 32 OR-ed in when the actor isn't in its threat list (`0.4` [17]–[60], `0.5` [48]). Without a type 3 the level stays 0, so the values are 0 and 32. An actor the pass finds without a check gets one a second later (`0.7`), which then repeats at randomized intervals; each check sends type 5 with the value whenever it differs from property 1338 (`prop_move_sync_filter`, sync `client`), and sets 1338 to it (`0.3` [7]–[47]). Two branches change nothing in M4, where the level is 0 and there are no teams: every second check first ANDs the value with the main chunk's 32, dropping the level bits (`0.3` [23]–[24]; the decompiled script shows the operand is the 32 loaded at `0` [12], which nothing else sets), and a team member's value is 0 for one mastery type of the local player and otherwise keeps its level only where the row has `include_team` (`0.4` [44]–[57]). What a value asks of the server isn't in the client: that 32 asks for less about an actor not engaged with the player is **Inferred**. In the runs each client sent one 32 for the other player, 2.8–3.4 s after its 7, and no other value (§13). | **Observed** (bytecode, runs), **Inferred**, **Unknown** (the server's side) | S20; M4S4, M4S5 |
| 14.13 | **Fenji's use of type 5.** A 5 with 0 is a pull for an id the client was told of and hasn't got, or the filter going to 0 for one it shows (a target selected). The world answers a pull only for a pid it announced and hasn't sent, so every 0 goes to the world as a pull. Fenji drops the other values: M4 has no combat or threat, and nothing to send less of. It never sends type 3. | the choices are Fenji's | 14.3, 14.12 |
| 14.14 | **Types 10 and 11 aren't needed in M4.** Type 10 unpacks a blob into an actor the client has (S2 `0.12`; S3 `0.49`), and nothing a player shows changes in M4: a player that an admin's `!tp` moves is taken away from its viewers (9) and announced again (6). After a 5, the scripts wait for nothing but the pulled 7, so Fenji sends no type 11 (`SYNC_UPDATE_FILTER`). | **Inferred** | S2 `0.12`; S3 `0.49`; S20 `0.3` |

What Fenji sends for seeing (the M4 world plan's Amendment B):

| The world says, or the client sends | Fenji sends | Values |
|---|---|---|
| `Announce { pid, pos }` | type 6 | the pid; x, y and z rounded to the nearest integer and clamped to `i32`; the flag and the priority 0 |
| type 5 with 0 | `WorldIn::Pull { conn, pid }` | other values, and bodies that aren't 18 bytes, go nowhere |
| `Enter { pid, look, pos, speed }` | type 7, then type 8 | the player's own 7 for that character (§5, §11), then the pid |
| `Leave { pids }` | type 9 | the pids as `u64` LE, in order |
| a 6, 7 or 9 naming the connection's own pid | nothing for that pid | 14.11 |

**Golden vectors**, bodies in hex:
- Type 6, pid 258 at the default start (2366, −71735, 64854), 22 bytes: `02 01 00 00 00 00 00 00 3e 09 00 00 c9 e7 fe ff 56 fd 00 00 00 00`.
- Type 6, pid 2^32 + 1 at (2.5, −2.5, 3 × 10^9), rounded and clamped: `01 00 00 00 01 00 00 00 03 00 00 00 fd ff ff ff ff ff ff 7f 00 00`.
- Type 5, the pull of pid 258: `02 01 00 00 00 00 00 00 00 00 00 00 01 00 00 00 00 00`.
- Type 5, the value 32 for pid 2^32 + 1, with byte 17 set: `01 00 00 00 01 00 00 00 20 00 00 00 01 00 00 00 00 01`.
- Type 5 as the real client sent it in m4-s4, with byte 17 zero: the pull of pid 2, `02 00 00 00 00 00 00 00 00 00 00 00 01 00 00 00 00 00`, and its 32, `02 00 00 00 00 00 00 00 20 00 00 00 01 00 00 00 00 00` (`crates/fenji-server/tests/fixtures/m4-s4-*.hex`).
- Type 9, pids 258 and 2^32 + 1: `02 01 00 00 00 00 00 00 01 00 00 00 01 00 00 00`.
- Type 7 for another player: V7 (§11).

## 15. Two clients on one machine

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 15.1 | **A second copy may run.** At start (FUN_14024F010) the client skips its instance check only when its command line has `--allow_multi`. The check (FUN_14024E790) creates the named mutex `GJMut1`; if that exists, it closes it and creates `GJMut2`; if that exists too, the client returns −4 and ends. So two copies run at once and a third doesn't, and no run needs `--allow_multi`. Neither the shared project nor REA finds a read of the name the check stores at 0x143114E10: its only references are the check's two writes, at 0x14024E7F5 and 0x14024E811. In every Task 13 run two copies ran at once without the flag (§13). | **Observed** (runs); the names, the flag, −4 and the references **Observed** | X15; M4S0–M4S5 |
| 15.2 | **Its other named objects are the web view's**: two queues, `c2s_webui_<t>` and `s2c_webui_<t>`, each shared memory with a `<name>_lock` mutex (FUN_140183DD0), where `t` is `_time64` in seconds when the web view starts (FUN_140188970); its helper `browserhost.exe` gets `--queueid=webui_<t>`. Two clients whose web views start in the same second in one prefix would share them; the runs saw the web view start on entering the world (§13), and in m4-s4 and m4-s5 both clients' web views started, 14 s and 52 s apart, each with its own `t`. Every other mutex, semaphore and event the exe creates is unnamed, and its one `FindWindowA` looks for a developer tool's window (`vResourceViewer`) to send log lines to. | **Inferred**; the strings **Observed** | X17, X18 |
| 15.3 | **Named objects are per prefix.** Wine keeps them in the prefix's wineserver, so two clients in one prefix see each other's `GJMut1` (the second takes `GJMut2`), and clients in two prefixes see nothing of each other. | **Inferred** (Wine's design) | 15.1 |
| 15.4 | **The log is in the exe's folder**: `%s/gujianol.log`, opened with `"w"`, in the folder `GetModuleFileNameA` gives (FUN_140245560, FUN_1400B6490). Two clients from one copy would truncate and interleave one log, so each needs its own copy. | **Inferred**; the format string **Observed** | X16 |
| 15.5 | **So two clients share one prefix, each from its own reflink copy of the client**, the second started some seconds after the first. One wineserver hosts Wine's own drivers from before the first client starts, so the watchdog's rule on a `winedevice.exe` started after the client keeps its meaning, and its rules on new `.sys` files and on the registry watch one prefix. Each client also needs its own nested display, since rootful Xwayland has no window manager: two client windows on one display stack, and XTEST input and screenshots reach only the top one. And each needs its own account, since a second login of one account replaces the first. **Task 13's rehearsal and runs answered what was Unknown here:** GE-Proton's `proton run` serves two clients in one prefix, under one wineserver; a second `proton run` brings no wineboot, services.exe or `winedevice.exe`, with or without a client; and the second web view starts. Whether two `umu-run` sessions do the same, which the bare runner's two-client mode rests on, is still **Unknown**. The runners' design is the M4 world plan's Amendment B. | the choices are Fenji's; **Observed** (runs); **Unknown** (`umu-run`) | §12 question 7; M4RH, M4S0–M4S5 |
| 15.6 | **A client's `proton run` waits for the other's programs.** In the rehearsal the second `proton run` started `cmd.exe /c exit`, which exited at once, and then stayed up, logging nothing, until the sandbox's timeout: Proton's shim waits for the session's other programs. So each client's `proton run` returns at its own timeout or once no client is left. The runner ends client 2's `gujianol.exe` by its own pid 10 s after its timeout, and finds it by its `argv[0]`: the client's main thread renames itself `vx_initialize` 1.1 s after start, and Wine passes thread names to Linux, so its process name changes (m4-s0). | **Observed** (runs), **Inferred** (the shim's wait) | M4RH, M4S0 |
| 15.7 | **In one Wine desktop the clients' input crosses.** With both clients in the default desktop, a key press on either display reached that desktop's foreground client: client 1's Return took client 2 off its title (m4-s0), and in m4-s1 and m4-s2 client 2 took none of its own clicks. So client 2 runs in a Wine desktop of its own, `explorer.exe /desktop=fenji-client2,1280x720` on its display. Rehearsed without a client, that start brought one `explorer.exe` and no new wineserver, wineboot or `winedevice.exe`. Then each display's input reached its own client, interleaved within seconds (m4-s4, m4-s5). | **Observed** (runs), **Inferred** (the foreground rule) | M4S0–M4S2, M4RD, M4S4, M4S5 |
| 15.8 | **On a nested display the focus follows the pointer.** Rootful Xwayland has no window manager, so the focus stays PointerRoot and a key press goes to the window under the pointer. A client in the world left the pointer on the display's bottom row, outside its window, (426, 719) after m4-s2 and (748, 719) after m4-s4, so the next run's key presses reached nothing until a click put the pointer over the window (m4-s3, and m4-s4 until its amended click). Client 2's Wine desktop gives its own window the focus. | **Observed** (runs; the pointer and the focus read with `XQueryPointer` and `XGetInputFocus`, which send nothing) | M4S3, M4S4 |

## 16. Moving

The client moves its own player and tells the server what it did; the other clients replay it. Movement is native code written for both ends, which one flag tells apart (16.2), and every move is a command with a time. Each client runs every actor's commands in 16 ms steps on one clock, the server's time as the type-2 replies set it (16.5).

| Step | Direction | Message | What happens | Evidence |
|---|---|---|---|---|
| 1 | — | the scripts call `actor_move_dir` every frame | a new direction command only when something changed, at most one per 50 ms (16.6) | S25 `0.24`; X22 |
| 2 | C→S | type 8 `CTS_PROTOCOL_MOVE_DIR`, 200 bytes | when the command starts: its time, the player's state then (where it stands) and the direction (16.3, 16.4) | X21; §13 |
| 3 | S→C | type 13 `STC_PROTOCOL_MOVE_DIR`, to each client that shows the player | queued at the command's time; when it starts, the player is put where its header says, then moved in its direction at its speed (16.8, 16.9) | X19, X20, X24 |
| 4 | S→C | type 20 `STC_PROTOCOL_MOVE_SYNC_SVR`, to the mover | "at time T you stood here": the client takes the position into its state of T and replays its own commands since (16.12) | X26 |

Type 8's body, 200 bytes:

| Offset | Bytes | Content | Fenji | Tag |
|---|---|---|---|---|
| 0 | 8 | the command's time, `u64` LE: the server's Unix time in ms as the client estimates it, a multiple of 16 (16.5) | the relay's time, bounded (16.15) | **Inferred**; offsets **Observed** |
| 8 | 12 | x, y and z, three `i32` LE: where the player stands when the command starts | the position | the same |
| 20 | 4 | a state word | not read | the same |
| 24 | 1 | a state byte | not read | the same |
| 25 | 2 | the yaw, `i16` LE, degrees within (−180, 180] | the position's yaw | the same |
| 27 | 4 | a state word that picks the last block's form: 0 in every run, whose bodies were all 200 bytes | checked: 0 with 200 bytes, not 0 with 185 (16.4) | the same |
| 31 | 134 | four state blocks of 22, 44, 20 and 48 bytes (33 for the last when the word at 27 isn't 0) | not read | the same |
| 165 | 1 | the direction: 0 stops; 1 to 8 name the yaw offset by eighths of a turn (16.7) | bounded to 0–8 | the same |
| 166 | 2 | the yaw again, `i16`: the command's own, which equals the yaw at 25 (16.4) | read for the trace alone: a relay takes the position's yaw | the same |
| 168 | 2 | the yaw offset, `i16`, degrees | folded into (−180, 180] | the same |
| 170 | 2 | the speed rate, `u16`: 100 is 500 units a second | bounded to 0–340 | the same |
| 172 | 4 | two pitches, `i16` each, degrees | each bounded to −90–90 | the same |
| 176 | 1 | the mode: 3 walks | not read | the same |
| 177 | 4 | the part of x below one unit, `f32` | not read | the same |
| 181 | 1 | a fact about the client's own process | neither read nor logged | the same |
| 182 | 4 | the part of y below one unit, `f32` | not read | the same |
| 186 | 8 | the gravity, property 1330 | not read | the same |
| 194 | 2 | a value the client clears at each send | neither read nor logged | **Unknown** (meaning) |
| 196 | 3 | two facts about the client's own process, a byte and a `u16` | neither read nor logged | **Inferred** |
| 199 | 1 | 1 when the client's own probe found the way blocked | not read | **Inferred** |

Type 13's body as Fenji sends it, 42 bytes; every type from 13 to 19 starts with its id and header:

| Offset | Bytes | Field | Fenji sends | Tag |
|---|---|---|---|---|
| 0 | 8 | actor id, `u64` LE | the mover's pid | **Inferred**; offsets **Observed** |
| 8 | 1 | the header's form, `i8`: above 0 absolute (as below); otherwise a `u16` time and four `i16` deltas from the last absolute header for that actor | 1 | the same |
| 9 | 8 | the command's time, `u64` LE | the move's bounded time | the same |
| 17 | 12 | x, y and z, `i32` LE | the checked position, rounded and clamped to `i32` | the same |
| 29 | 2 | the yaw, `i16` LE | the checked yaw, rounded and folded into (−180, 180] | the same |
| 31 | 1 | the direction | the bounded direction | the same |
| 32 | 2 | the yaw, `i16` | the yaw at 29 again | the same |
| 34 | 2 | the yaw offset, `i16` | the bounded yaw offset | the same |
| 36 | 2 | the speed rate, `u16` | the bounded rate | the same |
| 38 | 4 | two pitches, `i16` each | the bounded pitches | the same |

Type 20's body as Fenji sends it, 32 bytes:

| Offset | Bytes | Field | Fenji sends | Tag |
|---|---|---|---|---|
| 0 | 8 | actor id, `u64` LE | the player's own pid | **Inferred**; offsets **Observed** |
| 8 | 8 | the time T, `u64` LE | the server's time in ms, floored to a multiple of 16, less 160 (16.12) | the same |
| 16 | 2 | the mask, `u16` LE: which state fields follow | 0x00C0: the position (0x40) and the yaw (0x80) | the same |
| 18 | 12 | x, y and z, `i32` LE (mask 0x40) | the position, rounded and clamped to `i32` | the same |
| 30 | 2 | the yaw, `i16` LE (mask 0x80) | the yaw, rounded and folded into (−180, 180] | the same |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 16.1 | **Types 12 to 29 go to native code.** FUN_140222E00 hands 12 to FUN_14008E470, which returns at once, so the client ignores type 12. It hands 13 to 19 and 27 to 29 to FUN_14020FAA0 with a kind each: 13 a direction (3), 14 a move to a point (4), 15 a skill cast (7), 16 a jump to a point (1), 17 a stop (5), 18 a turn (6), 19 the end of a charge (8), 27 to 29 kinds 10 to 12. 20 goes to FUN_14020D100 and 26 to FUN_14020CCE0. | **Observed** (the switch, the empty function) | X1; X19; X26; X27 |
| 16.2 | **One move code for both ends.** The move module's settings (FUN_14020E090, copied to DAT_143C47CC0 by FUN_14016A6C0) start with a flag that the client sets to 0. The commands' writers add fields when the flag is 0, and their readers read those fields only when it isn't: in the client, a type 8 carries 24 bytes that a type 13 doesn't (16.4). That the flag marks the server's build is **Inferred**. | **Observed** (the flag, the branches), **Inferred** (its meaning) | X20; X23 |
| 16.3 | **Type 8 is the player's direction command.** `actor_move_dir` (FUN_1401FFB80, bound at 0x14020A413) makes a command (FUN_1401FC450) with the direction, the mode, the yaw, the yaw offset, the speed rate and two pitches. When the command becomes the player's current one, the step hook (FUN_140202930) calls FUN_1401FEE60, which writes the command's time, the player's state at that moment (copied by FUN_14016F280, written by FUN_14016D390 with every mask bit) and the kind's part (FUN_140170870), and sends it with FUN_140223380: kind 3 as type 8, kind 7 (a skill cast, jumps included) as 9, kind 8 as 10. It sends only for the player's own actor, the move state's flag at +0x112. Nothing else is sent as a move: a stop is a type 8 with direction 0. | **Inferred**; the types **Observed** | X20; X21; X22; X24 |
| 16.4 | **The layout above** follows FUN_14016D390's order (x, y and z are the low 32 bits of the state's 64-bit position; the yaw its low 16 bits) and the direction writer's, with the 24 bytes of 16.2: 8 + 109 + 48 + 35 = 200 bytes, the length of every type 8 in Task 11's runs (§13). When the word at 27 isn't 0, the last state block is 33 bytes and the body 185. The fractions at 177 and 182 are the parts of x and y below one unit, which the physics keeps (16.9). **The yaw at 25 is the command's, at 166:** when the command becomes the player's current one, its run (FUN_140170300) sets the actor's yaw to the command's, and the step hook sends the state after that (FUN_140202930, through FUN_1401FEE60), so a relay that repeats the yaw at 25 carries the command's own. Fenji's trace shows both, and in Task 15's runs they were equal in all 144 type 8s. | **Inferred**; the length and the two yaws' equality **Observed** (runs) | X20; X21; X24; §13; §16's runs |
| 16.5 | **The clock.** Command times come from FUN_140202910: the client's estimate of the server's time (FUN_140222DE0, which the type-2 replies set, §7) plus DAT_1431F3790, which is 0 in the image and is only read in Ghidra's analysis. The handler of 701 `PROTOS2C_MOVE_TIME_OFFSET` calls `set_move_time_offset`, which no script defines and the exe doesn't name. FUN_1401771A0 rounds a time up to the next multiple of 16, and the queue (FUN_140177240) drops a command whose time isn't one. So command times are the server's Unix time in ms, multiples of 16. | **Observed** (instructions), **Inferred** (the offset stays 0) | X10; X22; X23; X24; S22 `0.114` |
| 16.6 | **How often the client sends.** The scripts call `actor_move_dir` every frame (S25 `0.24` [18], [314], [357]), and it makes a command only when the player starts or stops (at once), or, at most once per 50 ms (`0x32`), when the yaw, the direction or the pitch changed or the current command's expiry (16.9) is less than 500 ms away. So a player standing still sends nothing, a player moving straight sends at least one a second, and no client sends more than one per 50 ms besides starting and stopping. In Task 11's runs one type 8 came about 1.6 s after the player's own 8, which fits the first frame's stop (the time of the last command, DAT_1431F37A8, starts at 0), none while the character stood, and 2 to 3 a second, 350 to 450 ms apart, while it ran. m4-e1, which drew the player dead, sent none; why is **Unknown**. In Task 15's runs (below), each held key made a type 8 when the player started, one every 0.5 to 0.8 s while it moved, and one with direction 0 when it stopped, often followed by a second direction 0 0.4 to 0.9 s later; each client's first type 8 came 1.7 to 3.0 s after its own 8. | **Observed** (instructions; runs), **Inferred** | X22; S25; §13; §16's runs |
| 16.7 | **The fields' ranges.** `actor_move_dir` truncates the yaw and the yaw offset to integers and folds them into (−180, 180] (`fmodf` by 360), caps the speed rate at the player's own speed (the move handle's FUN_140208790: property 1325, through FUN_140207D80) and passes the direction as a byte. The scripts' directions are 0 to 8: `yaw_offset_to_dir` (S25 `0.11`) gives the yaw offset by eighths of a turn, 1 at 0°, 5 at 45°, 3 at 90°, 7 at 135°, 2 at 180°, 8 at 225°, 4 at 270° and 6 at 315°. The speed rate is `GetLogicSpeed` (property 1325, less an offset when moving back, capped by 1327; S26 `0.25`), and the pitch is the camera's, in degrees, within the scope's limits (S25 `0.5`). The yaw is in degrees: `cos` and `sin` take yaw × 0.017453292. In Task 15's runs W sent direction 1, S 2 (offset 180), A 3 (offset 90) and D 4 (offset −90); W, A and D sent rate 100 and S rate 40, 200 units a second (`GetLogicSpeed`'s backward offset); a run that met a wall or was turning sent offsets and rates in between (offsets 126 to 159 with rates 23 to 35, or −14 with 24). | **Observed** (instructions, bytecode; runs), **Inferred** | X22; X25; S25; S26; §16's runs |
| 16.8 | **A relay puts the mover where its header says.** FUN_14020FAA0 reads a type 13 for an actor the client has into a command with its network flag set (+0x115); FUN_140176FA0 queues it unless the actor already has 100 queued; FUN_140177240 takes it however late. When its step comes, the step hook copies the header's x, y, z and yaw into the actor's state and applies them (FUN_140202930). A command more than 16 s old is dropped (FUN_140177E30). A late one is applied by taking the actor back to the command's time and stepping it forward again (FUN_1401781A0), through the 16 s of history (1,000 steps of 16 ms) that every player's move handle keeps (FUN_14015CD10). A relay for an actor the client doesn't have is dropped. In Task 15's runs every relay placed the mover where its type 8 said, though each type 8 came 0.13 to 2.01 s after its time, and each viewer showed the mover running and stopping where the mover's own client had it (m4-m1b, m4-m2, m4-m4). | **Inferred**; constants **Observed**; the effect **Observed** (runs) | X19; X24; §16's runs |
| 16.9 | **Then its direction moves it.** The direction command's run (FUN_140170300) sets the actor's yaw, its direction, its heading (yaw + yaw offset) and its speed: the rate × 500 / 100 units a second, the rate capped at the actor's own 1325. The command expires 1,500 ms after its time. Each 16 ms step (FUN_140177BF0) moves the actor speed × 16 × 0.001 units along the heading, keeping the fractions (FUN_140161060), and stops it once the expiry has passed. So others animate from the relays alone (world spec open question 4): each relay puts the mover back where it stood and moves it until the next, and a mover whose relays stop stops within 1.5 s. Direction 0 sets the speed to 0. Task 15's runs showed it: each viewer animated the other player from the relays alone, and in m4-m3 a viewer that got no relay for the mover's refused stop ran it on from its last relay, past where the server had it, until its next relay placed it again. | **Observed** (constants; runs), **Inferred** | X20; X24; X25; §16's runs |
| 16.10 | **Units.** The move code's positions are integers: the truncated `f32` of property 16 (FUN_1401FF980), so they are in property 16's units, which §13's minimap showed are centimetres. The yaw is in degrees (16.7). Rate 100 is 500 units, and the step code makes that per second (16.9), so 6.4's "per second" is **Observed** now. `movement::RUN`, `YAW_LIMIT` and `COORDINATE_LIMIT` stand. | **Observed** (instructions; §13's runs) | X14; X24; §13 |
| 16.11 | **Stopping, turning and jumping.** A stop is a type 8 with direction 0, and its relay a type 13 with direction 0 (16.9). 17 `MOVE_STOP` (one byte after the header; FUN_140174350 clears the direction and the speeds) and 18 `MOVE_YAW` (four `u32`: a target yaw as an `f32`, a step count, a turn direction and one more; FUN_140174910) are the server's own commands. Jumps, dashes and glides are skill casts (`CastJumpSkill` and `CastDashSkill`, S26 `0.32` and `0.31`; L4): the client sends them as type 9 (kind 7: a skill id, a target, optional points, a name; FUN_140175140), and others receive them as 15 (FUN_140175590). In Task 11's runs, where the run hit rocks, the client said a skill wasn't learnt (§13): M4 sends no skills. In Task 15's runs Space sent a type 9 of 263 bytes, and in m4-m1b a type 10 of 165 bytes after it; Fenji ignores both, so no jump reaches another client, and neither client showed "Not yet learnt" in the shots. 14 and 16 carry target points for the server's scripted moves (FUN_14016FF20 reads 16's). | **Inferred**; the runs **Observed** | X28; S26; L4; §13; §16's runs |
| 16.12 | **20 `MOVE_SYNC_SVR`.** FUN_14020D100 reads an actor id, a time T, a `u16` mask and the masked state (FUN_14016E880: x, y and z as `i32` with 0x40, the yaw as `i16` with 0x80), and acts unless the actor's property 242 `prop_move_cli_moving` is set (FUN_140207900). If the actor's history holds T (FUN_140161E90: no more than 1,000 steps after the history's start, which stays 8 to 16 s behind the actor's clock) and the actor ran that step, it takes the masked fields into the state of T (FUN_14016CCF0) and steps forward again to now. A T the actor hasn't reached yet is recorded (FUN_140177880) and applied when its step comes (16.8's hook); a T before the history's start is ignored. For the player's own actor it also lets the next direction command go out at once (16.6), as type 23 does (6.2). So T is best a little in the client's past: Fenji uses its clock floored to 16 ms, less 160 ms. Only two buffs' scripts set 242 (S27), and Fenji gives no buffs and doesn't send 242, so in M4 it stays unset. A far 20 makes the client run its own scene teleport, which the scripts' actor update runs when the player's position changes by 200 m or more within one update (S3 `0.43` [66]–[80]; S1 `0.36`): in Task 16's t16-c3 a 20 that moved the player 374 m within scene 3006 brought the client's loading screen for about three seconds, then the player stood at the 20's place, with no 772 and no new type 3 ([`chat.md`](chat.md) §7); a near one (1.9 m, t16-c1) moved the player at once. That the 200 m check caused it is **Inferred**; no 20 between the two was tried. In m4-m3 (a build for that run only that refused moves ending over 5 m from the start) each 20 had T 165 to 173 ms behind the server's clock, and the client took it: a player who had stopped stood at the 20's place, and its next command started there; a player still holding a run ran on from it, since the run's command is replayed from T, and its client sent nothing more for that run, so its next command started where the replay had taken it. | **Inferred**; constants **Observed**; the effect **Observed** (m4-m3; t16-c1, t16-c3) | X24; X26; S1; S3; S27; §16's runs; [`chat.md`](chat.md) §7 |
| 16.13 | **26 `MOVE_ENTER_SYNC_STATE`** (FUN_14020CCE0): an actor id, a mask and the masked state, set at once on another player's actor, never the player's own. Fenji doesn't need it: 7 places an entering player (14.8), and its next relay places it again. | **Inferred** | X27 |
| 16.14 | **419 `PROTOC2S_SET_RUN_STATE`.** The walk toggle (S25 `0.23`) calls `SetRunState` (S18 `0.250`), which sends 419 with a boolean and changes nothing itself. Walking is a 1325 below 60, which makes the mode 3 (S25 `0.24` [352]–[354]), and only the server sets 1325. Fenji ignores 419 in M4: players always run. In Task 15's runs the toggle sent 419, and the runs after it kept their rates, 100 forward and 40 back. | **Observed** (bytecode; runs), the choice is Fenji's | S18; S25; §16's runs |
| 16.15 | **What Fenji takes from a type 8.** The position: x, y, z and the yaw, unbounded, since the world checks them (speed, limits, finiteness). The rest, each bounded: the time clamped to the server's clock less 5,000 ms to plus 1,000 ms (Task 15's runs: real clients' commands came up to 2.01 s late, and the first bound, 2,000 ms, clamped one; §16's runs) and floored to a multiple of 16; the direction to 0–8; the yaw offset folded into (−180, 180]; the speed rate to 0–340 (`MAX_RATE`); each pitch to −90–90. Nothing else: the bytes at 181, 194, 196 and 197 describe the player's machine (FUN_140201180 fills them, one from a byte of the client's export `curl_easy_exit` through FUN_140247D00), like 7.2's counters and 14.4's byte, so Fenji neither reads nor logs them, and its trace log shows the decoded move, never the body. A relay is built from these fields and the checked position only. | the choices are Fenji's | 16.4; X29 |
| 16.16 | **Queries 6 and 7.** The client sends type 6 (a `u32` 0 or 1, a `u32` count, then actor ids as `u64`) and type 7 (a `u32` count, then actor ids) about actors its scripts list for checking, at most every 1,600 and 3,000 ms (FUN_140201180, with FUN_14020F6A0, FUN_14020FE90 and FUN_14020B5C0). Fenji ignores them in M4. Neither client sent a 6 or a 7 in Task 15's runs, with the other player in view and moving. | **Inferred**; the intervals **Observed**; none in the runs **Observed** | X29; §16's runs |

What Fenji sends for moving (the M4 world plan's Amendment C):

| The client sends, or the world says | Fenji sends | Values |
|---|---|---|
| type 8 | `WorldIn::Move`: the position, and the rest bounded (16.15) | a body that isn't 200 bytes with 0 at 27, or 185 with another word there, goes nowhere |
| types 6, 7, 9, 10 and 11; script message 419 | nothing | 16.11, 16.14, 16.16 |
| `Moved { pid, step }` | type 13, to each client that shows the mover | the header from the checked position and the move's bounded time, the direction part from the bounded fields; at most one a tick for each mover, its last accepted. Only direction commands reach the world in M4; once casts and jumps do (9, 15), only direction commands may coalesce: a newer move never replaces one |
| `Correct { pos }` | type 20, to the mover | T = the server's time floored to 16 ms, less 160 ms; mask 0x00C0, the position and the yaw; at most one a tick, with where the mover stands after the tick's moves |
| `Teleport { pos }` | type 20, to the player | the same |
| a 13 naming the connection's own pid | nothing | 16.8: it would place the player's own actor |

**Golden vectors**, bodies in hex, with T = 1,791,936,000,000 ms (`00 70 b5 37 a1 01 00 00`):
- Type 8, the player at the default start (2366, −71735, 64854) with yaw −15, direction 1, yaw offset 0, rate 100, pitches −10 and 10, made from the layout above for Fenji's tests; 200 bytes, by segment:
  - 0–19: `00 70 b5 37 a1 01 00 00 3e 09 00 00 c9 e7 fe ff 56 fd 00 00`
  - 20–30: `00 00 00 00 00 f1 ff 00 00 00 00`
  - 31–164: 134 bytes of `00`
  - 165–175: `01 f1 ff 00 00 64 00 f6 ff 0a 00`
  - 176–199: `00 00 00 80 3e 5a 00 00 00 3f 14 00 00 00 00 00 00 00 5a 5a 5a 5a 5a 00`
- Type 13, that move relayed for pid 258, 42 bytes: `02 01 00 00 00 00 00 00 01 00 70 b5 37 a1 01 00 00 3e 09 00 00 c9 e7 fe ff 56 fd 00 00 f1 ff 01 f1 ff 00 00 64 00 f6 ff 0a 00`.
- Type 20, pid 258 put at the same point with the server's clock at T, 32 bytes: `02 01 00 00 00 00 00 00 60 6f b5 37 a1 01 00 00 c0 00 3e 09 00 00 c9 e7 fe ff 56 fd 00 00 f1 ff`.

The plan's Amendment C has the bounded and the hostile vectors.

Open questions for Task 15's runs, each run's prediction written before it starts. The runs below answered 1, 3 and 4, and the refused-move half of 2:
1. **Seeing each other move** (16.8, 16.9): two clients walk, run, turn and stop near each other; each shows the other moving and stopping where its own client shows it. **Answered** (m4-m1b, m4-m2, m4-m4): yes.
2. **Corrections and teleports** (16.12): a refused move puts the player back where the server has it (with a build for that run only whose check refuses moves, as m4-e3 had its own); and, once Task 16 puts chat on the wire, an admin's `!tp` within the scene, near and beyond 200 m, puts the player at the target and shows whether a far one runs the client's scene teleport. **The refused-move half answered** (m4-m3): the 20 puts the player back at T, and the client replays its own commands since T from there. **The `!tp` half answered** (Task 16's t16-c1 and t16-c3, [`chat.md`](chat.md) §7): a `!tp` is a 20 at the target's place; 1.9 m away the player moved at once, and 374 m away, within scene 3006, the client showed its loading screen for about three seconds and then stood the player beside the target, so a far one runs the client's scene teleport (16.12).
3. **Jumping and walking** (16.11, 16.14): a jump shows that the skill isn't learnt and sends no type 9; the walk toggle sends 419 and changes nothing. **Answered, in part otherwise** (m4-m1, m4-m1b): a jump does send a type 9 (263 bytes), then a type 10 (165 bytes), and no client showed "Not yet learnt" in the shots; the toggle sends 419 and changes no rate.
4. **Queries** (16.16): whether either client sends type 6 or 7 about the other, and whether ignoring them changes what it shows. **Answered** (m4-m1 to m4-m4): neither client sent a 6 or a 7, with the other player in view and moving.

### Runs (Task 15)

Task 15's runs (2026-10-15) ran two clients at once as Task 13's did (§13), with `run-client.sh run --clients 2`, against `fenji-server` built from the task's branch, with the watchdog armed. Each run used its own copy of Task 13's database, seeded on the host before the run's first start: Lin (pid 1, `tester`) entered at `[world]`'s start, Mai (pid 2, `second`) at a seeded place. The players moved by held keys (`--key T:KEY/MS`), on the client's own bindings (S28): W forward, S back, A and D sideways, Space to jump, keypad `/` to toggle walking. The server logged each type 8 decoded, with both yaws, at trace, and each 13 and 20 at debug; host shots of both displays came every one or two seconds while the players moved. Predictions were written before each run (M4R15).

| Run | What the run did | What happened | Watchdog | Evidence |
|---|---|---|---|---|
| m4-m1 | Mai at Task 13's seed, 2 m up the path; each runs back, forward and sideways, Lin jumps and toggles walking; Mai leaves first | every type 8 decoded, and each that came while the other client had the mover reached that client as a 13 within a tick, but two of Lin's that came in one tick, which went out as one 13, the later; **Lin moved, but Mai never left his seed**: his client sent his directions with the seed's place each time; the host shots failed | no sign | M4M1 |
| m4-m1b | as m4-m1, with Mai seeded where Lin's client had stood | **each client showed the other run, turn and stop where the other's own client had it**; 55 type 8s, 54 13s, each pairing exactly; the 55th, Lin's first, came before Mai entered | no sign | M4M1B |
| m4-m2 | both enter again on m4-m1b's database, then run once each | **each came back where it stood**: its 772 and its first type 8 at its last place in m4-m1b; each client showed the other there, then running | no sign | M4M2 |
| m4-m3 | a build for this run only refuses a move that ends over 5 m from the start; Lin runs out of that, and Mai starts outside it | five 20s, each T 165 to 173 ms back, each at the world's last place for its player, and no refused move relayed; Lin's client took hers; Mai's replayed his held run from it | no sign | M4M3 |
| m4-m4 | the final build; one run back and forth each | as m4-m1b: 19 type 8s, 18 13s, each pairing exactly; each client showed the other run | no sign | M4M4 |

**Observed (Task 15):**
- **Seeing each other move** (question 1). Each accepted type 8 reached the other client as a 13 within a tick (0.1 to 55 ms, median 26 ms, 133 relays), with its position and its bounded fields. In the shots each client showed the other run back, forward and sideways, turn to each direction and stop, a second or two behind the mover's own screen, and at rest each showed the other where the other's own client had it: in m4-m1b at 390 s client 1's minimap read (27, −724) for Lin, whose last type 8 put her at (2769, −72350), and client 2's (31, −720) for Mai at (3120, −71937), and each client's view put the other there. No client logged an error for a 13 or a 20.
- **The type 8s.** All 200 bytes (16.4), 144 in the five runs. Both yaws were equal in every one (16.4). Their times lagged the server's clock by 0.13 to 2.01 s at their receipt and never led it; one of the 144 lagged past 2 s, which the first bound of 2 s clamped (16.15).
- **Coming back** (Review Focus 1). A client that left was saved at its last type 8, place and yaw, and its next session's 772 and first type 8 were there (m4-m1b to m4-m2).
- **Corrections** (question 2's first half). The fence of m4-m3 refused five type 8s, and each brought one 20 with the world's last place for the player, T a multiple of 16 and 165 to 173 ms behind the server's clock; no refused move reached the other client. Lin's stop was refused: her client took the 20's place, 3.1 m back, and her next command started there. Mai's held run was refused at its start: his client took the 20 at T, ran on from there, and sent nothing more for that run; his next command started 4.3 m on, inside the fence, and the world took it.
- **Jumping, walking and queries** (questions 3 and 4) as above.
- **Mai's seed in m4-m1**, (2559, −71787, 64854), computed in Task 13, kept him in place while his client sent his runs; Lin stood 6 units higher than the start's z (64860), and every later seed was a place a client had reported.

**Inferred (Task 15):**
- **The lag** is the clients' frame rate: two clients on lavapipe ran at a few frames a second, and a client sends a command only when the frame that steps it comes (16.3).
- **m4-m1's seed lies inside the stairs' collision**, so every step was blocked.
- **After a refused move a viewer shows the mover where its last accepted relay took it**, up to 1.5 s of movement past where the server has it (16.9), until its next accepted move; an honest client's next command comes when it next starts, turns or stops.

**Unknown (Task 15):**
- Whether the jumper's client played the jump: the 2-s shots fell around it.
- Why Mai's client sent no update or stop for its refused run in m4-m3.

## Evidence index

`re/` is git-ignored, so the script listings, caches and notes exist only on the owner's machines. `luatool` regenerates the listings from the pinned build. A script key names the disassembly `re/m1/disasm/data/<path>.luac.dis` and the decompiled `re/m1/decompiled-all/data/<path>.lua`, with the function paths cited. `PG/` stands for `data/locale/common/core/modules/pregame/`, and `AS/` for `data/locale/common/interface/addons/ActorSelect/`.

| Key | Path or address | What it holds |
|---|---|---|
| S1 | `data/core/client/scene_manager.luac` | The main chunk `0`: 772's closure into register 62 [105] and the stubs for 772, 92, 330, 394, 490 and 733 [112]–[135]. The 772 handler (`0.56`), the `maps` lookup (`0.51`), `cli_loadmap` (`0.54`), `scene_load_scene` (`0.37`), the load update that calls `scene_start_load` (`0.47`), the local player's entry hook (`0.49`), `scene_is_in_world` (`0.19`), `scene_get_type` (`0.5`), the current tid (`0.8`) and map (`0.13`), the open-world test (`0.18`), `scene_require_teleport` (`0.36`: `allow_tele` [1], the scene teleport's event [45]) |
| S2 | `data/core/client/net_manager.luac` | The type handlers for 2 (`0.7`), 6 (`0.8`), 7 (`0.9`), 8 (`0.10`), 9 (`0.11`) and 10 (`0.12`), put in the handler table in the main chunk `0` [75]–[88]; `send_gateway` (`0.22`), `send` (`0.25`), `send_enter` (`0.26`), `send_leave` (`0.27`), `send_time_sync` (`0.28`), `net_manager_send_sync_filter` (`0.29`); `net_manager_enable_send` (`0.31`) and `net_manager_disable_send` (`0.32`) |
| S3 | `data/core/client/actor_manager.luac` | Which actors get a move handle (`0.20`), its creation (`0.21`), the handles (`0.24`), creating an actor (`0.33`), destroying one (`0.34`, from `actor_system_destroy_actor`, `0.46`), the actor by id (`0.39`), `actor_system_set_local_id` (`0.42`), `actor_system_enter_finish` (`0.45`), `actor_system_unpack_props` (`0.49`), `actor_system_reset_props` (`0.51`), `actor_system_create_actor` (`0.52`), `actor_system_local_player_enter` (`0.53`), `actor_system_get_local_id` (`0.82`), `actor_system_update` (`0.43`: the local player's position, read at [36]–[41] and again at [66]–[68], the squared distance against 400,000,000.0 [77], `scene_require_teleport` [79]) |
| S4 | `data/core/client/time_manager.luac` | `SERVER_TIME_SYNC_INTERVAL` (`0`), `time_manager_update` (`0.1`), `time_manager_time_sync_resp` (`0.2`) |
| S5 | `data/core/client/main.luac` | The per-frame function (`0.9`), which calls `time_manager_update` |
| S6 | `data/interface/system/UIMain/UIMain.luac` | The `STATE_*` constants (`0`), the net-status timer `OnTimer_CheckNetStatus` (`0.3`), `UpdateLoadScene` (`0.9`), `OnRequestLoadScene` (`0.24`), `OnLocalActorEnter` (`0.30`); `OnActorLogout`, which `SCENE.EVENT_LOGOUT` calls, and `ReturnSelectActor` (1.5), by name |
| S7 | `data/core/modules/scene/scene.luac` | `NotifySceneLoaded` (`0.61`) and `RequestLogout` (`0.62`) |
| S8 | `data/core/xobject/client_actor_class_register.luac` | The main chunk `0`: BUILTIN_PROPS and `prop_tables` merged, the sort, `__ACTOR_PROPERTIES_VERSION`, and the registration by type [275]–[455]; the `table` getter and setter (`0.5`, `0.6`), the `locale_str` setter (`0.8`), the `any` setter (`0.10`) |
| S9 | `data/core/client/actor_id_sync.luac` | `actor_id_enter` (`0.13`): the 2^48 test |
| S10 | `PG/pregame_actor.luac`, `PG/pregame.luac`, `AS/ActorSelect.luac` | The preview actor's `create` (`0.0`): its properties and `actor_system_create_actor(pid, 1, 255, vocation × 10000, …)`; its `destroy` (`0.3`); `ClearActor` (`pregame.luac` `0.21`), which the select screen's `OnUnLoad` calls (`ActorSelect.luac` `0.13`); `ActorSelect.luac`'s `ReturnSelectActor` and `ShowSelectActor` (1.5), by name |
| S11 | `data/core/modules/model/player_model_handle.luac` | The model's `on_handle_init` (`0.26`), set at `0` [112] |
| S12 | `data/locale/common/interface/addons/ActorCreate/CreatePanel.luac`, `data/interface/addons/ActorCreate/CreatePanel.luac`, `data/core/modules/vocation/vocation.luac` | The only uses of `prop_base_equips_id` (the 304 request); `GetVocationPreviewEquips` (`0.20`), the one reader of the outfit sheet; a search of every listing for `CteateEquips` finds nothing |
| S13 | `data/core/share/enum_base.luac` | The main chunk `0` [1030]–[1054]: `EQUIP_SLOT_HEAD` 1 to `EQUIP_SLOT_WEAPON` 11 |
| S14 | `data/core/modules/system/system.luac` | `RequestQuitGame` (`0.5`): 392 through `send_gateway` |
| S15 | `data/core/share/item_logic/item_category.luac` | The equipment test on `id >> 24` (`0.0`) and the id from type and number, `type << 24` (`0.23`, `0.24`) |
| S16 | `data/core/modules/net/net.luac` | The close-reason table in the main chunk `0` (decompiled lines 1674–1810), as in [`lobby.md`](lobby.md) S1 |
| S17 | `data/core/modules/player/player.luac` | `OnActorEnter`, for other players (`0.2`); the local player's setup, `OnLocalActorEnter` (`0.4`); `OnHPChanged` (`0.20`) and the model's update with the `PostDead` effect (`0.21`) |
| S18 | `data/core/client/actor_interface.luac` | `GetHPRat` (`0.47`), `GetHP` (`0.48`), `GetHPMax` (`0.50`); `SetRunState` (`0.250`), which sends 419 [8] |
| S19 | `data/core/client/actor_id_sync.luac` | The three queues and their timers (main chunk `0`, [69]–[89]), the enqueue (`0.8`), the dequeue (`0.9`), the pick (`0.10`), the timer's pull (`0.11`), `actor_id_enter` (`0.13`), `actor_id_leave` (`0.14`); the debug history (`0.0`–`0.3`), which `actor_id_on_actor_sync` (`0.15`) only adds to |
| S20 | `data/core/client/actor_filter.luac` | The main chunk `0`: 3000 ms [1], 32 [12], the level rows, the threat rank 10 [39]; the check that sends type 5 (`0.3`), the values for players (`0.4`) and NPCs (`0.5`), the 3-second pass (`0.6`), `actor_filter_update` (`0.7`), `actor_filter_set_lv` (`0.8`) |
| S21 | `data/core/share/timer_service.luac` | `timer_service_insert` (`0.0`) and the update that sets a timer with an interval again (`0.6`) |
| S22 | `data/core/combat/client/combat_system.luac` | `on_actor_enter_finish` (`0.143`), exported as `combat_on_actor_enter_finish` in the main chunk `0` [480]–[496]; the handler of 701 `PROTOS2C_MOVE_TIME_OFFSET` (`0.114`), set in `0` [370], which calls `set_move_time_offset` [6] |
| S23 | `data/interface/addons/BillBoard/BillBoard.luac`, `BillBoardActor.luac` | `OnLoad` (`BillBoard.luac` `0.17`): name boards on `SCENE.EVENT_ACTOR_ENTER` [33] and `SCENE.EVENT_ACTOR_LEAVE` [38]; the properties a board watches (`BillBoardActor.luac` main chunk `0` [89]–[109]) |
| S24 | `data/core/modules/transmit/transmit_with_effect.luac` | `RegisterEvents` (`0.16`): the one listener of `SCENE.EVENT_ACTOR_ENTER_FINISH` [11], `OnActorEnterScene` (`0.14`) |
| S25 | `data/core/modules/control/control_move.luac` | `move.runwalk` (`0.23`): `SetRunState` [2]–[3] with the opposite of property 475 [7]; `move.after` (`0.24`): its three calls of `actor_move_dir` ([18], when the player can't move; [314], the gamepad's; [357], the keyboard's), `GetLogicSpeed` [346], 1325 [352] and the walk test against 60 [354]; the pitch (`0.5`): the scope's `control_pitch_min` [4] and the camera's angle [20]; `yaw_offset_to_dir` (`0.11`) and its table (`0` [50]–[62]); both `move` functions set in `0` [113], [115] |
| S26 | `data/core/modules/control/logic_move.luac` | `GetLogicSpeed` (`0.25`): 1327 [17]; `CastDashSkill` (`0.31`) and `CastJumpSkill` (`0.32`); set in the main chunk `0` [63], [75], [77] |
| S27 | `data/scripts/combat/buff/1667.luac`, `data/scripts/combat/buff/4259.luac` | The only setters of property 242 in the scripts: each sets it to `true` (`0.0` [34]) and back to `false` (`0.2` [25]). A search of every listing for the constant 242 finds no other setter; the rest read it or use 242 for something else |
| S28 | `data/core/modules/input/input_default_keys.luac` | The default key bindings, read in the decompiled text: `move.forward` W, `move.backward` S (214), `move.left` A (196), `move.right` D (199), `move.jump` Space (163), `move.runwalk` the keypad's `/` (222), `move.auto_move_switch` G (202); and the key codes' names (`CT_KB_*`) |
| D1 | `data/caches/protocol_ver.new_cache` | The protocol table: 772 `PROTOS2C_ACTOR_LOAD_SCENE`, 92, 330, 394, 10, 392 |
| D2 | `data/caches/prop_tables.new_cache` | 2,439 rows: name, type, sync class and default |
| D3 | `data/caches/all_tables.new_cache` | `vocation_initequip_templ` (`CteateEquips`, `PreviewEquips`), `vocation_templ` (`CreateShowEquipHead`, `creator_info`), `maps` |
| D4 | `data/caches/locale_en.new_cache` | The English texts of the close reasons and of UIMain's fallbacks |
| X1 | FUN_140222E00 | Inbound messages by type: case 2 (0x140222E59), case 6 (0x140222F3E), case 7 (0x140222FAE), case 8 (0x14022306B), case 9 (0x140223006), case 10 (0x14022304F), and cases 21 and 23; the move types 12 to 20 and 26 to 29 (16.1) |
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
| X12 | FUN_140116840 (`lua_pushinteger`), FUN_140116AA0 (`lua_pushboolean`), FUN_140116EF0 (`lua_createtable`), FUN_140117630 (`lua_rawseti`) | The Lua calls X1's cases 6 and 9 push with |
| X13 | FUN_140221D30 (`send_sync_filter`), FUN_140119940 (`luaL_checkinteger`), FUN_140246EA0, FUN_141157040 (called through 0x1411569E0), the stubs at 0x14029990C | Type 5's body |
| X14 | FUN_1401FF980 (`actor_move_create`, bound at 0x14020A3BC), FUN_14018D230, FUN_14018D2E0 | A move handle's start, from property 16 |
| X15 | FUN_14024F010, FUN_14024E790; 0x143114E10 and its references, the writes at 0x14024E7F5 and 0x14024E811 | `--allow_multi`, `GJMut1` and `GJMut2`, and the stored name, which no reference reads |
| X16 | FUN_140245560, FUN_1400B6490 | `gujianol.log` in the exe's folder |
| X17 | FUN_140188970, FUN_140183DD0; the string at 0x141161898 | The web view's named queues and its helper's `--queueid` |
| X18 | The exe's import table, read with a standard-library PE reader: the slots of `CreateMutexW` (0x141159518), `OpenMutexW` (0x141159510), `CreateMutexA` (0x1411596E0), `CreateSemaphoreA` (0x1411596D8), `CreateSemaphoreW` (0x1411595F0), `CreateEventA` (0x141159790), `CreateEventW` (0x1411592A8) and `FindWindowA` (0x14115A648). Their references, in the shared project and through REA, lie in the callers: of `CreateMutexW`, FUN_14024E790 (twice), FUN_140183BE0 and FUN_140183DD0; of `OpenMutexW`, FUN_140183BE0 and FUN_140183DD0; of `CreateMutexA` and `CreateSemaphoreA`, FUN_140A96340; of `CreateSemaphoreW`, FUN_140B0EAE0; of `FindWindowA`, FUN_140A92AE0. `CreateEventA`'s slot has 14 references and `CreateEventW`'s one; each call's name argument, 0, was read in the shared project | The exe's named objects |
| X19 | FUN_14020FAA0, FUN_1401FF030, FUN_140176FA0; the kinds' vtables from 0x1411894A8 (the base, the jump to a point, the direction, the move to a point, the stop, the cast and the end of a charge) | Types 13 to 19 and 27 to 29: the id, the header (absolute or delta), the kind's part, and the queue's cap of 100 |
| X20 | FUN_140170870 (the direction part's writer), 0x140170C90 (its reader, which Ghidra's analysis made no function of; read with `objdump`), FUN_140171040 (its log), FUN_140170300 (its run) | The direction part, and what running it does |
| X21 | FUN_1401FEE60, FUN_140223380, FUN_14016F280, FUN_14016D390 | The client's moves, types 8, 9 and 10, and the state they carry |
| X22 | FUN_1401FFB80 (`actor_move_dir`, bound at 0x14020A413), FUN_1401FC450, FUN_1401771A0 | When a direction command is made, its fields' ranges, and its time |
| X23 | FUN_14020E090, FUN_14016A6C0, FUN_140202910; DAT_1431F3790 and its references | The move module's settings and their flag, and its clock |
| X24 | FUN_140177240, FUN_140177E30, FUN_1401781A0, FUN_140177BF0, FUN_140161060, FUN_140202930, FUN_140177880, FUN_14015CD10 | The command queue, the 16 ms step, the history and the rollback, and the step hook, which places a relayed actor and sends the player's own commands |
| X25 | FUN_140208790, FUN_140207D80, FUN_140207160, FUN_140207B80; the move handle's vtable at 0x1411898E0 | The speed (1325) and the gravity (1330) the move code reads |
| X26 | FUN_14020D100, FUN_14016E880, FUN_14016CCF0, FUN_140161E90, FUN_140207900 | Type 20 |
| X27 | FUN_14020CCE0, FUN_14008E470 | Types 26 and 12 |
| X28 | 0x140174400 (the stop's reader, which Ghidra's analysis made no function of; read with `objdump`), FUN_140174350; the turn's vtable at 0x14118C528 and FUN_140174910; FUN_140175140, FUN_140175590, FUN_14016FF20 | The stop, the turn, the cast and the jump to a point |
| X29 | FUN_140201180, FUN_140247D00; the intervals at 0x143114670; FUN_14020F6A0, FUN_14020FE90, FUN_14020B5C0 | The facts about the client's process in type 8, and the queries, types 6 and 7 |
| L1 | `re/m4/entering/notes.md`, with its REA bundle in `re/m4/entering/rea/` | This reading's notes: every REA call and its ID, the unknowns recorded in REA, and what the local probes printed |
| L2 | `re/m4/entering/builtin_props.tsv` | BUILTIN_PROPS dumped from the exe with a standard-library PE reader |
| L3 | `re/m4/entering/vocation_sheets.json` | `vocation_initequip_templ` and `vocation_templ`, decoded from D3 with a standard-library reader |
| L4 | `re/m4/world-survey/` | The design-time survey (entering, sync and movement, chat, the start and outfit): the leads this reading checked |
| L5 | `re/m4/seeing/notes.md`, with its REA bundles in `re/m4/seeing/rea/` | §14 and §15's notes: every REA call and its ID, the unknowns recorded in REA, the bytecode positions and the import table |
| L6 | `re/m4/movement/notes.md`, with its REA bundles in `re/m4/movement/rea/` | §16's notes: every REA call and its ID, the unknowns recorded in REA, the bytecode positions, the bytes read with `objdump`, and the runs' type-8 counts |
| M4E1 | `re/runs/out/m4-e1/`, with screenshots after 295 s in `re/m4/t11/m4-e1/` | Task 11's first world run (§13): `server.log` (trace), `gujianol.log`, `wine.log`, screenshots and the watchdog's log |
| M4E2–M4E4 | `re/runs/out/m4-e2/` to `m4-e4/`, with screenshots in `re/m4/t11/m4-e2/` to `m4-e4/` | The same for runs m4-e2 to m4-e4 |
| M4R | `re/m4/t11/runs.md` | The runs' predictions, written before each run, and their outcomes |
| M4RH | `re/m4/t13/rehearsal/` | Task 13's rehearsal (15.5, 15.6): its script, `io/ps.txt`, both Proton logs, the registry and `.sys` diffs, `scanlog` of each log |
| M4RD | `re/m4/t13/rehearsal-desktop/` | The desktop rehearsal (15.7): the same for client 2's own Wine desktop, and a screenshot of `:97` |
| M4S0 | `re/runs/out/m4-s0/`, with host screenshots in `re/m4/t13/m4-s0/` | Task 13's first two-client run: `server.log` (trace), both clients' `gujianol.log` (client 2's in `client2/`), `wine.log` and `wine-2.log`, screenshots, the watchdog's log |
| M4S0B, M4S0C | `re/runs/out/m4-s0b/`, `re/runs/out/m4-s0c/` | The single-client runs that made Lin and Mai |
| M4S1–M4S5 | `re/runs/out/m4-s1/` to `m4-s5/`, with host screenshots in `re/m4/t13/m4-s1/` to `m4-s5/` and enlarged views in their `zoom/` | The same for runs m4-s1 to m4-s5; m4-s4's inputs added by hand in `re/m4/t13/m4-s4-amendment.log`, and the read-only display query in `re/m4/t13/xstate.py` |
| M4R13 | `re/m4/t13/runs.md` | Task 13's predictions, written before each run, and their outcomes |
| M4M1 | `re/runs/out/m4-m1/`, with host screenshots in `re/m4/t15/m4-m1/` | Task 15's first movement run: `server.log` (trace: every type 8 decoded; debug: every 13 and 20), both clients' `gujianol.log`, `wine.log` and `wine-2.log`, screenshots, the watchdog's log |
| M4M1B, M4M2, M4M3, M4M4 | `re/runs/out/m4-m1b/`, `m4-m2/`, `m4-m3/`, `m4-m4/`, with host screenshots in `re/m4/t15/m4-m1b/` to `m4-m4/` and the montages read in their `strips/` | The same for runs m4-m1b to m4-m4; m4-m3's build-only patch in `re/m4/run/m4-m3-movement.rs.fence` and its binary's hash in `re/m4/run/m4-m3-fence-build.sha256` |
| M4R15 | `re/m4/t15/runs.md` | Task 15's predictions, written before each run, and their outcomes |

### REA Evidence IDs

Read through REA 6.3.0 with Ghidra 12.1.4 on the pinned exe, in six sessions on 2026-10-14: the second for eleven IDs the first had missed, the third for §14 and §15, the fourth for the references behind 15.1 and X18, the fifth and the sixth for §16; each reading shows the function in the role the finding gives it. The first, the third and the fifth session's first call, on FUN_140222E00, returned the ID [`../opcodes.md`](../opcodes.md) gives for it, and the third's FUN_140221D30 did too. The fourth's `xrefs` on 0x141161898 and its FUN_14024E790 returned the third's IDs below, and the fifth's FUN_1401FF980 returned X14's. An ID covers its call exactly as sent, so anyone can check it by repeating the call ([`../../re/README.md`](../../re/README.md#how-to-cite)). The IDs for FUN_140221AC0, FUN_140246E40, FUN_140246DF0 and FUN_1400B5670 are in [`../transport.md`](../transport.md), and FUN_1400D5850's in [`../payloads.md`](../payloads.md).

| Function or address | Cited in | Call | Evidence ID |
|---|---|---|---|
| FUN_140222E00 | X1; 4.1, 6.1, 7.3, 14.1, 14.11, 16.1 | `procedure_pseudo_code {procedure: "0x140222e00"}` | `ev_4656519e08dab96fa5ad38e228a578375ef1b30907fb075530e4d9e15b4d3d20` |
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
| FUN_140222DE0 | X10; 7.3, 16.5 | `procedure_pseudo_code {procedure: "0x140222de0"}` | `ev_3942b189e3a5091a06c50210797945a849c3e9f63fd186cc7299caffeb724c9d` |
| the exe's strings | X11; 9.1 | `search_strings {pattern: "equips_id", mode: "literal"}` | `ev_b83f8a725bb3c9d6fe73c0a77111a20a8521532dd4e2a43e2d1d1133f8a3631e` |
| FUN_140116840 | X12; 14.1 | `procedure_pseudo_code {procedure: "0x140116840"}` | `ev_a9f123899e37112c336e28be5645c51ea7696fa8411df0a0080e2055a8c3a7ff` |
| FUN_140116AA0 | X12; 14.1 | `procedure_pseudo_code {procedure: "0x140116aa0"}` | `ev_13cae7695fc7d0f898562b8d388082829a79fa661f064ef3fd7d95d7f36c27f7` |
| FUN_140116EF0 | X12; 14.11 | `procedure_pseudo_code {procedure: "0x140116ef0"}` | `ev_4179452c05f8dae36b0be30e4745d3c8ec213f3ee39de0cab2c17586de8c60ac` |
| FUN_140117630 | X12; 14.11 | `procedure_pseudo_code {procedure: "0x140117630"}` | `ev_36c69b25bc382b1740f99265034dac7c895de4ae99c7d3d370a4568e2d05d546` |
| FUN_140221D30 | X13; 14.4 | `procedure_pseudo_code {procedure: "0x140221d30"}` | `ev_eba0244ecb8c94a02f7e0ad89c87cc89fc99e5e9a426d10e11f69e7a85bd3b30` |
| FUN_140119940 | X13; 14.4 | `procedure_pseudo_code {procedure: "0x140119940"}` | `ev_317d15fdb99cc5d272e8006c9debd13a747baee82473b8578482a06c58c8bed9` |
| FUN_140246EA0 | X13; 14.4 | `procedure_pseudo_code {procedure: "0x140246ea0"}` | `ev_595f9b951e97473ff677478c466f84c2ae089995dfcc46669c006dceb7072964` |
| FUN_141157040 | X13; 14.4 | `procedure_pseudo_code {procedure: "0x141157040"}` | `ev_c1a01ca2fa792404ee4c5d5a81bf96bb73be068e209c9295e16747b0014c3aaa` |
| 0x14029990C | X13; 14.4 | `read_bytes {address: "0x14029990c", length: 44}` | `ev_19c0f12e05e26567ca6461aaaed42b169e21f250661329a6b6736a2dca551206` |
| FUN_1401FF980 | X14; 14.8, 16.10 | `procedure_pseudo_code {procedure: "0x1401ff980"}` | `ev_b78db3f77126a938d48184527c8b92ddafeaa9b641926142a8eac9a037c181df` |
| 0x14020A3BC | X14; 14.8 | `inspect_native_instruction {address: "0x14020a3bc"}` | `ev_3112a012a31d69195a8d3602799b83c3d69918fc0b039a1d9707bcd8400f8094` |
| FUN_14018D230 | X14; 14.8 | `procedure_pseudo_code {procedure: "0x14018d230"}` | `ev_c6ff080c51f73928c20a74fb16b3b4a0a7094ac03539fe11c3f63900e57390cf` |
| FUN_14018D2E0 | X14; 14.8 | `procedure_pseudo_code {procedure: "0x14018d2e0"}` | `ev_51b82cee7c67af4b06ee01c95aa0b1e8bdd88e94a63e583db2db6d7b8489eec2` |
| FUN_14024F010 | X15; 15.1 | `procedure_pseudo_code {procedure: "0x14024f010"}` | `ev_a3fcb94db8ff9db03a63277fd36998063ab52d2143c39e1bbae5184f4dfe39a4` |
| FUN_14024E790 | X15, X18; 15.1 | `procedure_pseudo_code {procedure: "0x14024e790"}` | `ev_6bb7c3fb7a10c5fcfdf5bae8a699bd3c0d42449de33c3f260d3eff9078610d6f` |
| FUN_140245560 | X16; 15.4 | `procedure_pseudo_code {procedure: "0x140245560"}` | `ev_e3a5ba6f2d95cf7eb9fcde3f5c839e3b2b8eb025a32d464feb078362aa503894` |
| FUN_1400B6490 | X16; 15.4 | `procedure_pseudo_code {procedure: "0x1400b6490"}` | `ev_99fb299335db19ebf5904b16fd4658b06beae3a95349774e007bd4a28156f86a` |
| FUN_140188970 | X17; 15.2 | `procedure_pseudo_code {procedure: "0x140188970"}` | `ev_1f6e131d0bfdc8aa7330e46ab43e3939d99d8008d0442243eab6ecb8fc80be22` |
| FUN_140183DD0 | X17; 15.2 | `procedure_pseudo_code {procedure: "0x140183dd0"}` | `ev_3a6ac622b039c18d37629baeb394075fd0d1de0a1567c7be6108a5ae2b550e70` |
| the exe's strings | X17; 15.2 | `search_strings {pattern: "c2s_webui_", mode: "literal"}` | `ev_c433afc86d18817a3e4d1ac7e2365362af0aed845c263f611921b92754363ba9` |
| 0x141161898 | X17; 15.2 | `xrefs {address: "0x141161898"}` | `ev_89c5acdc19b29a20d8fda57fb1911fe1fd71d605ef53fabcc5785337db2c0965` |
| 0x143114E10 | X15; 15.1 | `xrefs {address: "0x143114e10"}` | `ev_ccf84e3184afac6dbd752911cfa655caa17846170a886613e697052cb56a0d68` |
| 0x14024E7F5, a write | X15; 15.1 | `inspect_native_instruction {address: "0x14024e7f5"}` | `ev_87c9457d455e1aaacd31bc81dbcacc364ffd91740f6e44195ab807803ecfb0a2` |
| 0x14024E811, a write | X15; 15.1 | `inspect_native_instruction {address: "0x14024e811"}` | `ev_c5e1d30f92983f1d643b0b0af6335313303c3c4c9ed360ae72b2817dc71977b0` |
| `CreateMutexW`'s slot, 0x141159518 | X18; 15.2 | `xrefs {address: "0x141159518"}` | `ev_ef0ebead76e8bac94f27e0e50ee1934566e03573e4cc1735bd7ae99efe4dd59c` |
| `OpenMutexW`'s slot, 0x141159510 | X18; 15.2 | `xrefs {address: "0x141159510"}` | `ev_72097583c4eb4ef43903dbb0797997970a3d57b9294fd4f45cb06505037a140a` |
| `CreateMutexA`'s slot, 0x1411596E0 | X18; 15.2 | `xrefs {address: "0x1411596e0"}` | `ev_f33e204b14823f4a48e36a67e6d4efdf8d22d6262253d9f683141c509b2a4dea` |
| `CreateSemaphoreA`'s slot, 0x1411596D8 | X18; 15.2 | `xrefs {address: "0x1411596d8"}` | `ev_b11386c2ecf3efbbfea4d4a3f72301505f35d558f58d83238b6339c93700bd8c` |
| `CreateSemaphoreW`'s slot, 0x1411595F0 | X18; 15.2 | `xrefs {address: "0x1411595f0"}` | `ev_e6229c721b1425eecb0cf12d0bd208642107c9e447fb09c986f1aa29ee8c9640` |
| `CreateEventA`'s slot, 0x141159790 | X18; 15.2 | `xrefs {address: "0x141159790"}` | `ev_ce6e112c72ae40fa4fa08e56d7550b6dcd6cdb01a0dd0298d094c02f0b7a57b5` |
| `CreateEventW`'s slot, 0x1411592A8 | X18; 15.2 | `xrefs {address: "0x1411592a8"}` | `ev_24f7db8ae4d46963bc8e1235f79342348782512d3729e6283e0da3dbb7765c61` |
| `FindWindowA`'s slot, 0x14115A648 | X18; 15.2 | `xrefs {address: "0x14115a648"}` | `ev_720b0736e4735f669d6b4e7f13804732a6c59c09cd7a2f81b51eb1329cc889ac` |
| 0x140183CD9, in FUN_140183BE0 | X18; 15.2 | `resolve_containing_procedure {address: "0x140183cd9"}` | `ev_9314fee3b8cd6536ba82be4d7c2d2833655b5db9fd01d67cb78662e640fa7829` |
| 0x140183D23, in FUN_140183BE0 | X18; 15.2 | `resolve_containing_procedure {address: "0x140183d23"}` | `ev_b4deccc45f872b00ce117191db74926aa937740183cbeb695381f2adace5c94f` |
| 0x140183ED2, in FUN_140183DD0 | X18; 15.2 | `resolve_containing_procedure {address: "0x140183ed2"}` | `ev_75a17ab0a31b6d6ad2584a11e3f6fe79680a424c0380cae3c815a3bee0eb5f61` |
| 0x140183F21, in FUN_140183DD0 | X18; 15.2 | `resolve_containing_procedure {address: "0x140183f21"}` | `ev_fa1507fdf7ddd490f8612a63e840ac40db4dcda3aeaca70fae532aca9e01ec11` |
| 0x140A963AF, in FUN_140A96340 | X18; 15.2 | `resolve_containing_procedure {address: "0x140a963af"}` | `ev_7ef6fdfdbfaca974af78ca04f3109fcaebf7376109aa496fc3937e10d9905e91` |
| 0x140A963DE, in FUN_140A96340 | X18; 15.2 | `resolve_containing_procedure {address: "0x140a963de"}` | `ev_22a5a766024f2f6ede2d67ef43873fceaf610e77b361784d745a78cd8fb96582` |
| 0x140B0EAEE, in FUN_140B0EAE0 | X18; 15.2 | `resolve_containing_procedure {address: "0x140b0eaee"}` | `ev_9c055d38f93e399c3258485c82490ee3724bd592cb413f54d222c217798833ee` |
| 0x140A92B56, in FUN_140A92AE0 | X18; 15.2 | `resolve_containing_procedure {address: "0x140a92b56"}` | `ev_bea2377d1a9932fd9a0cd3db96fa80ca1db61d8889ff83a5d5c3d3efdf5b913c` |
| FUN_14020FAA0 | X19; 16.1, 16.8 | `procedure_pseudo_code {procedure: "0x14020faa0"}` | `ev_99e89bdb404a053e255ebf19d4ed3f2e3ca02445c696c2075c2cda368ac84fe4` |
| FUN_1401FF030 | X19; type 13's table, 16.8 | `procedure_pseudo_code {procedure: "0x1401ff030"}` | `ev_e173f3a01ceb98903e2b333d38a514e7449310da0b850eeb6d2c1fada7cd4938` |
| FUN_140176FA0 | X19; 16.8 | `procedure_pseudo_code {procedure: "0x140176fa0"}` | `ev_cde2345a4f162abedeb3d3d4634fbedbc8d7d981aaa7652ddcf92cd778b6188f` |
| 0x1411894A8 | X19; 16.1 | `read_bytes {address: "0x1411894a8", length: 768}` | `ev_86df7508df4dce1146ebef413ecb8b4a764fb74820edaface9a291525814a907` |
| FUN_140170870 | X20; 16.2, 16.3, 16.4 | `procedure_pseudo_code {procedure: "0x140170870"}` | `ev_6e26ada75f630180d47c389006931d5038143e20efc5cc38842b2dd1fd95e7d8` |
| 0x140170C90 | X20; 16.2, 16.4 | `read_bytes {address: "0x140170c90", length: 932}` | `ev_e4b6627fad1e14bc7edc428971406d297ea34e97dd67184d1ea22a5ed7007678` |
| FUN_140171040 | X20; type 8's table | `procedure_pseudo_code {procedure: "0x140171040"}` | `ev_f85fc137a9607e4f893e9dc7ccd5528b63b69ac3ba3ca3f2b8334b425f41e6ee` |
| 0x140171040 | X20; type 8's table | `read_bytes {address: "0x140171040", length: 86}` | `ev_906e13a896f9e990a9bf58c706247451590f554a903256b85d19869fb0ffbe12` |
| FUN_140170300 | X20; 16.4, 16.9 | `procedure_pseudo_code {procedure: "0x140170300"}` | `ev_ea5fcb7451c34eac334dbbf5dc5a58c8e232006541218788210feef6ccae8ec6` |
| FUN_1401FEE60 | X21; 16.3, 16.4 | `procedure_pseudo_code {procedure: "0x1401fee60"}` | `ev_5c61cdb14fe5e0860d3fc9d5bf68c98d3112090c747a6d4d3e4ef495465ca0ba` |
| FUN_140223380 | X21; 16.3 | `procedure_pseudo_code {procedure: "0x140223380"}` | `ev_9142b701da3b012c27ceef58777fd79daa830bf55a5989ca4a817f838173cf3c` |
| FUN_14016D390 | X21; 16.3, 16.4 | `procedure_pseudo_code {procedure: "0x14016d390"}` | `ev_736f67207c4769e6f4d843223f4f75eeaa0850f57fa57e6ac867f13b990f8059` |
| FUN_14016F280 | X21; 16.3 | `procedure_pseudo_code {procedure: "0x14016f280"}` | `ev_50c8c82b32efd94311c9cd6f9d6dfa6fbd8087d20467cc48d71dbb094dee2d9b` |
| FUN_1401FFB80 | X22; 16.3, 16.6, 16.7 | `procedure_pseudo_code {procedure: "0x1401ffb80"}` | `ev_368adcb76b068796489bcd423b1e3674dbe2e686a9d531f56ceabd462a8aaa4f` |
| 0x14020A413 | X22; 16.3 | `inspect_native_instruction {address: "0x14020a413"}` | `ev_f9e9739d07676f1359c21aa3736b5be74550d3167cf353c4dd03412401394109` |
| FUN_1401FC450 | X22; 16.3 | `procedure_pseudo_code {procedure: "0x1401fc450"}` | `ev_e8ddd10e2fa96801f39e7706d1cecfabccbfe474d8591715a34c05c3f17dc1bd` |
| FUN_1401771A0 | X22; 16.5 | `procedure_pseudo_code {procedure: "0x1401771a0"}` | `ev_e1f42f10f9e519a28eb2729daa523b40e7dc8e8b5904ac523cf5a37b6345e557` |
| FUN_14020E090 | X23; 16.2 | `procedure_pseudo_code {procedure: "0x14020e090"}` | `ev_1a8d51ad986b80d02b45425afdc3176ca58ec08c92471d65d9049254385ed531` |
| FUN_14016A6C0 | X23; 16.2 | `procedure_pseudo_code {procedure: "0x14016a6c0"}` | `ev_24b1e2d4bfc389ca1f682d87dadc6319979fcce6144c4396f971db153bef4cde` |
| FUN_140202910 | X23; 16.5 | `procedure_pseudo_code {procedure: "0x140202910"}` | `ev_213e3ae86691054461eba46c6fb203c2a5237a50e4d19821aa2ed4f4791f3ccd` |
| 0x1431F3790 | X23; 16.5 | `xrefs {address: "0x1431f3790"}` | `ev_88c8b56726e01b0d35c69f8098f6321ec83e75f25d2429cc1ad6bb7652e7270a` |
| FUN_140177240 | X24; 16.5, 16.8 | `procedure_pseudo_code {procedure: "0x140177240"}` | `ev_ded2877931bde160d72f72cb9e12e88054264e872cf9b5cd6c6c317fa4d28c42` |
| FUN_140177E30 | X24; 16.8 | `procedure_pseudo_code {procedure: "0x140177e30"}` | `ev_afb7f46c59a4fdea85e80273e237cba50a80115ba43e37246d2804a960a06b15` |
| FUN_1401781A0 | X24; 16.8 | `procedure_pseudo_code {procedure: "0x1401781a0"}` | `ev_66f6e116bf5ffcf99291f3fbd04dccd158a27ba7f383e48c4e58fabeeebb6fcc` |
| FUN_140177BF0 | X24; 16.9 | `procedure_pseudo_code {procedure: "0x140177bf0"}` | `ev_4d511f79fad8989458ccf2a371e55716c5024d00852695c3f6768d74c585e571` |
| FUN_140161060 | X24; 16.9 | `procedure_pseudo_code {procedure: "0x140161060"}` | `ev_b879b34156eec817af065dc5455b6c8da851ced7ff825340e26ba273631c042e` |
| FUN_140202930 | X24; 16.3, 16.4, 16.8, 16.12 | `procedure_pseudo_code {procedure: "0x140202930"}` | `ev_1607850ce17e77ce533b8408479ba460d3995ff2369e6c850cbb92ed1c34db98` |
| FUN_140177880 | X24; 16.12 | `procedure_pseudo_code {procedure: "0x140177880"}` | `ev_9748093220b333e473ffe23a10b91d7ff9c9d813ad4122d51779c1d24366e049` |
| FUN_14015CD10 | X24; 16.8 | `procedure_pseudo_code {procedure: "0x14015cd10"}` | `ev_4d2164ae34fd08b24cb8939fed0d13d128e09d151eb52571ef98815bc4e81c3a` |
| FUN_140208790 | X25; 16.7 | `procedure_pseudo_code {procedure: "0x140208790"}` | `ev_9288deb200789d08df553d5b17c0f86db43c6dfc2628b69b2d231aa351a1ae3a` |
| FUN_140207D80 | X25; 16.7 | `procedure_pseudo_code {procedure: "0x140207d80"}` | `ev_885e49e024575bf1f78c6aab17b4ffa9ec88798ccff15698096e5195047076c7` |
| FUN_140207160 | X25; type 8's table | `procedure_pseudo_code {procedure: "0x140207160"}` | `ev_7f687702bae1dc26c6180a752d463c1963288bf104fbb3a3b0a848c68ec2d654` |
| FUN_140207B80 | X25; type 8's table | `procedure_pseudo_code {procedure: "0x140207b80"}` | `ev_fd83df95c4af2ff5bbfb89d8a08de8d284052dbb6b0fd49e7d523ce7db537725` |
| 0x1411898E0 | X25; 16.7 | `read_bytes {address: "0x1411898e0", length: 56}` | `ev_5d655be0e5c4ad030ca9af640d8dba254deb21b922ce65cd619be32662836b01` |
| FUN_14020D100 | X26; 16.1, 16.12 | `procedure_pseudo_code {procedure: "0x14020d100"}` | `ev_c77b3e4a688b193b47505c781b1326d6a5c7958eb4adac75eea7666673be60a6` |
| FUN_14016E880 | X26; 16.12 | `procedure_pseudo_code {procedure: "0x14016e880"}` | `ev_9c2b89f6842ab7418c8911bd5d969d61f43a0191c8d5005b196b0dfdbd7b2e7b` |
| FUN_14016CCF0 | X26; 16.12 | `procedure_pseudo_code {procedure: "0x14016ccf0"}` | `ev_a695f77f375f8660872697802442930704f74c5f60bd9f2cc8c035ba24ecc13a` |
| FUN_140161E90 | X26; 16.12 | `procedure_pseudo_code {procedure: "0x140161e90"}` | `ev_05bfcf284652370e10e949b5ad988499e591dd2fae0bab6754fe68d2bd5561cb` |
| FUN_140207900 | X26; 16.12 | `procedure_pseudo_code {procedure: "0x140207900"}` | `ev_ab73c2938a4a5e03853f407033026a7ece14024b3308e301031447f2044c8581` |
| FUN_14020CCE0 | X27; 16.1, 16.13 | `procedure_pseudo_code {procedure: "0x14020cce0"}` | `ev_b0a26738c1208ef35214e639ba5344b840b6a49f245dd0c4c7a4a1e65c80bf4a` |
| FUN_14008E470 | X27; 16.1 | `procedure_pseudo_code {procedure: "0x14008e470"}` | `ev_fdb9082d2b3e751d0f71d2498577e3f3c1c7ab5fbd8455d1b4a27e344dfddd39` |
| 0x140174400 | X28; 16.11 | `read_bytes {address: "0x140174400", length: 72}` | `ev_f3d09cc3390e80216ee352a04face64d537643c7c7538de8930fb0b3aa32317e` |
| FUN_140174350 | X28; 16.11 | `procedure_pseudo_code {procedure: "0x140174350"}` | `ev_15de1121cd35e809cff88c0222f6d9bc431ec8054265c1e566c93672369c35e5` |
| 0x14118C528 | X28; 16.11 | `read_bytes {address: "0x14118c528", length: 48}` | `ev_6ada2fd7a7399d20327c5e93266ed28cb5e9dff6f77823055a7ee7f7487cac2d` |
| FUN_140174910 | X28; 16.11 | `procedure_pseudo_code {procedure: "0x140174910"}` | `ev_d2131080f257aac0e5ee9605cdca76c14d276a9769f5b02e9a2a471133e0c4bc` |
| FUN_140175140 | X28; 16.11 | `procedure_pseudo_code {procedure: "0x140175140"}` | `ev_e5d6d2fd2aca8ea9e381da161e53bcb6c94c61d63189f6dc7e5ccff53e5ccc7f` |
| FUN_140175590 | X28; 16.11 | `procedure_pseudo_code {procedure: "0x140175590"}` | `ev_19f8947a518afec75fa396cec79ad0bb303e63eac2141bb413ee20632bb6db58` |
| FUN_14016FF20 | X28; 16.11 | `procedure_pseudo_code {procedure: "0x14016ff20"}` | `ev_63027d335638665f552c6605c18eee00fc16c797d3c9bd08efa0562e3cfbdb33` |
| FUN_140201180 | X29; 16.15, 16.16 | `procedure_pseudo_code {procedure: "0x140201180"}` | `ev_0633ddf606e96f59d52e5d05131d71673004523b04f9ddf57e1562306ad54b5d` |
| FUN_140247D00 | X29; 16.15 | `procedure_pseudo_code {procedure: "0x140247d00"}` | `ev_fbf02c313b5b2b467b3330a8fb631acf260226aed779314c3bc7c203cd76018b` |
| FUN_14020F6A0 | X29; 16.16 | `procedure_pseudo_code {procedure: "0x14020f6a0"}` | `ev_036175611b41eb57c77b35d727798475a1b8747c45ffdc4e1a1918a709f2442c` |
| FUN_14020B5C0 | X29; 16.16 | `procedure_pseudo_code {procedure: "0x14020b5c0"}` | `ev_4878f25434f019c125d1e98720ffc0ae27f1ff5e3079e39bd583309e190779ee` |
| 0x143114670 | X29; 16.16 | `read_bytes {address: "0x143114670", length: 16}` | `ev_d9dd4ba63c38ae4029734feabebad585835c47f821f4290676603a8e7bdac3ce` |
