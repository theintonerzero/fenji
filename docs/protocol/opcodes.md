# Opcodes

The inner message header's type byte ([`transport.md`](transport.md) §7) is the opcode. The client names its opcodes in two enums, `STC_PROTOCOL_*` (server to client) and `CTS_PROTOCOL_*` (client to server), listed in [spike-0](../re/spike-0.md) 4.12. This ledger gives each one's wire value. Most game traffic rides on the two script opcodes, `STC_PROTOCOL_SCRIPT` and `CTS_PROTOCOL_GATE_SCRIPT` / `CTS_PROTOCOL_SCRIPT`, whose bodies are pickled values: a script message id first, then its arguments ([`payloads.md`](payloads.md)).

**Method.** This ledger reads the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`) in the shared Ghidra project, read-only, together with the client's own scripts disassembled and decompiled locally by `luatool` (M1). Each function the findings O.1–O.3 cite was also read through REA, after the evidence norm ([`../re/README.md`](../re/README.md#the-norm)); the Evidence IDs are in [REA Evidence IDs](#rea-evidence-ids). Script evidence cites the script's engine path. Runs are the sandboxed ones in [`transport.md`](transport.md) §9, cited by their evidence keys there. It was written by a session that has not read any source rejected in [`../re/sources.md`](../re/sources.md). Statuses follow [`README.md`](README.md): `named` (the client's name is known, the payload isn't mapped), `schema` (payload mapped from the client's code), `verified` (seen against the real client in a sandboxed run and covered by a test).

## How the values were found

| # | Finding | Tag | Evidence |
|---|---|---|---|
| O.1 | **Server to client.** In states 2 and 3 the network thread (FUN_1402234D0) closes the connection on a message of type 1 (FUN_14029A870) and hands every other inbound message, type 0 included, to the scripts as an event. For each event, FUN_140222E00 pushes the type, then decodes the body by type in a switch over 0 to 29: type 0 is unpickled (FUN_1400D5850), type 1 pushed as a string, type 2 read as a `u32` and a `u64` (it also updates the ping estimate), and types 3 to 29 each have their own parser. Those cases line up with the 30 `STC_PROTOCOL_*` names in their table order (the 31st, `MAX`, is the count). | **Inferred** | FUN_1402234D0, FUN_140222E00 |
| O.2 | **Client to server.** The NetClientLua methods post send commands to the network thread, which writes the command's type byte into the message header (FUN_14029AC60): `send_gateway` uses 0xFF, `send` 0, `send_time_sync` 2, `send_enter` 3, `send_leave` 4 and `send_sync_filter` 5. That is `CTS_PROTOCOL_GATE_SCRIPT` as −1 (0xFF as a byte) and the rest of the enum counting from 0 in table order. | **Inferred**; the type constants **Observed** | FUN_140221940, FUN_140221A00, FUN_140221AC0, FUN_140221BB0, FUN_140221CB0, FUN_140221D30, FUN_1402226B0 |
| O.3 | The client sends only in state 3 `WORK` ([`transport.md`](transport.md) 7.3), and the scripts' senders check that state themselves (`net_manager_send_gateway`, `net_manager_send`; `data/core/client/net_manager.luac`). | **Inferred** | FUN_14029AC60; net_manager.luac |

## Server to client (`STC_PROTOCOL_*`)

| Value | Direction | Client name | Handler | Status | Evidence |
|---|---|---|---|---|---|
| `0` | S→C | `STC_PROTOCOL_SCRIPT` | unpickled (FUN_1400D5850), then the scripts' handler for the message id (`data/core/client/net_manager.luac`) | `verified` | O.1, [`payloads.md`](payloads.md); M2R4B, `crates/fenji-proto/tests/fixtures.rs` (`r4b_the_server_sends_what_the_client_accepted`) |
| `1` | S→C | `STC_PROTOCOL_CLOSE` | FUN_14029A870: the body (at most 255 bytes kept) becomes the close reason, the socket closes, state 5 | `schema` | O.1, [`transport.md`](transport.md) 7.4 |
| `2` | S→C | `STC_PROTOCOL_TIME_SYNC` | FUN_140222E00 case 2: a `u32` (the client's clock, echoed) and a `u64` (the server's time) feed the ping estimate and the scripts' `time_manager_time_sync_resp` ([`transport.md`](transport.md) 10.6) | `schema` | O.1, [`transport.md`](transport.md) 10.6 |
| `3` | S→C | `STC_PROTOCOL_TRANS_LIMIT_LV` | case 3: one `u16` | `named` | O.1 |
| `4` | S→C | `STC_PROTOCOL_DEBUG_POS` | case 4 | `named` | O.1 |
| `5` | S→C | `STC_PROTOCOL_GM_CHANNEL` | case 5: FUN_1401A49C0 | `named` | O.1 |
| `6` | S→C | `STC_PROTOCOL_SYNC_ID_ENTER` | case 6 | `named` | O.1 |
| `7` | S→C | `STC_PROTOCOL_SYNC_ENTER` | case 7 | `named` | O.1 |
| `8` | S→C | `STC_PROTOCOL_SYNC_ENTER_FINISH` | case 8: one `u64` | `named` | O.1 |
| `9` | S→C | `STC_PROTOCOL_SYNC_LEAVE` | case 9: an array of `u64` | `named` | O.1 |
| `10` | S→C | `STC_PROTOCOL_SYNC_UPDATE` | case 10 | `named` | O.1 |
| `11` | S→C | `STC_PROTOCOL_SYNC_UPDATE_FILTER` | case 11: FUN_1401AE640 | `named` | O.1 |
| `12` | S→C | `STC_PROTOCOL_MOVE` | case 12: FUN_14008E470 | `named` | O.1 |
| `13`–`19` | S→C | `STC_PROTOCOL_MOVE_DIR`, `_MOVE_POS`, `_MOVE_COMBAT`, `_MOVE_JUMP_POS`, `_MOVE_STOP`, `_MOVE_YAW`, `_MOVE_FINISH_CHARGING` | cases 13–19: FUN_14020FAA0 with a sub-kind each | `named` | O.1 |
| `20` | S→C | `STC_PROTOCOL_MOVE_SYNC_SVR` | case 20: FUN_14020D100 | `named` | O.1 |
| `21` | S→C | `STC_PROTOCOL_MOVE_BASE_PROP` | case 21: FUN_14020BA70 | `named` | O.1 |
| `22` | S→C | `STC_PROTOCOL_MOVE_SYNC_PATH_FIND_TASK` | case 22: FUN_14020EA60 | `named` | O.1 |
| `23` | S→C | `STC_PROTOCOL_SYNC_PROP_FORCE` | case 23: FUN_14020CE90 | `named` | O.1 |
| `24` | S→C | `STC_PROTOCOL_ACTOR_HMAP_MASK_REFRESH` | case 24: FUN_14020E680 | `named` | O.1 |
| `25` | S→C | `STC_PROTOCOL_OVERALL_HMAP_MASK_REFRESH` | case 25: FUN_14020E880 | `named` | O.1 |
| `26` | S→C | `STC_PROTOCOL_MOVE_ENTER_SYNC_STATE` | case 26: FUN_14020CCE0 | `named` | O.1 |
| `27`–`29` | S→C | `STC_PROTOCOL_MOVE_PASSIVE`, `_MOVE_SURROUND`, `_MOVE_FORMULAR` | cases 27–29: FUN_14020FAA0 | `named` | O.1 |

## Client to server (`CTS_PROTOCOL_*`)

| Value | Direction | Client name | Handler | Status | Evidence |
|---|---|---|---|---|---|
| `0xFF` | C→S | `CTS_PROTOCOL_GATE_SCRIPT` | `send_gateway` (FUN_140221940): the pickled message id and arguments, for the gate | `verified` | O.2, [`payloads.md`](payloads.md); M2R4B, `crates/fenji-proto/tests/fixtures.rs` (`r4b_client_gateway_402_decodes`) |
| `0` | C→S | `CTS_PROTOCOL_SCRIPT` | `send` (FUN_140221A00): the pickled message id and arguments | `schema` | O.2, [`payloads.md`](payloads.md) |
| `1` | C→S | `CTS_PROTOCOL_GM_CHANNEL` | — | `named` | O.2 |
| `2` | C→S | `CTS_PROTOCOL_TIME_SYNC` | `send_time_sync` (FUN_140221AC0): a 13-byte body, sent once a second once a character is in the world ([`transport.md`](transport.md) 10.6) | `named` | O.2 |
| `3` | C→S | `CTS_PROTOCOL_CLIENT_ENTER` | `send_enter` (FUN_140221BB0): a 21-byte body | `named` | O.2 |
| `4` | C→S | `CTS_PROTOCOL_CLIENT_LEAVE` | `send_leave` (FUN_140221CB0): an empty body | `schema` | O.2 |
| `5` | C→S | `CTS_PROTOCOL_SYNC_FILTER_SET` | `send_sync_filter` (FUN_140221D30): an 18-byte body | `named` | O.2 |
| `6`–`11` | C→S | `CTS_PROTOCOL_QUERY_ACTOR_POS`, `_QUERY_ACTOR_MOVE_CMD`, `_MOVE_DIR`, `_MOVE_COMBAT`, `_MOVE_FINISH_CHARGE`, `_MOVE_DEBUG` | by table order; their senders weren't read | `named` | O.2 |

The type-0 message the client sends during auth ([`transport.md`](transport.md) §6) isn't a script message: the client builds it directly in the hello handler, before state `WORK`.

## Script messages

Script message ids are numbers. In a shipping client, `data/core/share/enums.luac` takes each id and name from the client's own protocol table (`enum_protocols.luac`, which sets each `PROTOG2C_*`, `PROTOC2G_*`, `PROTOS2C_*` and `PROTOC2S_*` name from a global, is the developer path; [`packets/lobby.md`](packets/lobby.md) 2.1), so the names don't follow their order in that file. That table, `data/caches/protocol_ver.new_cache`, is where the names below come from ([`packets/lobby.md`](packets/lobby.md) 2.1). Client messages go as `CTS_PROTOCOL_GATE_SCRIPT` and server messages as `STC_PROTOCOL_SCRIPT`. Statuses are those of the tables above. The ids used so far:

| Id | Direction | Name | What it does | Status | Tag | Evidence |
|---|---|---|---|---|---|---|
| 414 | S→C, `STC_PROTOCOL_SCRIPT` | `PROTOG2C_SET_NET_CONNECTED` (the handler logs this name) | Moves the client to `WORK`: it stores billing fields, calls `set_working`, fires `EVENT_NET_CONNECTED` and sends 402 ([`transport.md`](transport.md) §10) | `verified` | **Inferred**; its effect **Observed** | `data/core/modules/net/net.luac` `0.29`; M2R4, M2R4B, M2R5; `crates/fenji-proto/tests/fixtures.rs` (`r4b_the_server_sends_what_the_client_accepted`) |
| 402 | C→S, `CTS_PROTOCOL_GATE_SCRIPT` | `PROTOC2G_SERVER_LIST` | Sent with no arguments once 414 is handled: in each run, 2.3 s after we sent 414. Needs no reply; 495 is optional ([`packets/lobby.md`](packets/lobby.md) §2) | `verified` | **Observed**; the name **Observed** in the protocol table | net.luac `0.29`; M2R4, M2R4B, M2R5; `crates/fenji-proto/tests/fixtures.rs` (`r4b_client_gateway_402_decodes`) |
| 292 | C→S | `PROTOC2S_ROLE_LIST` | Asks for the character list, after 370 and 496 (`QueryActors`); answered with 228 | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §2, §3 |
| 370 | C→S | `PROTOC2G_HOTPATCH_INIT` | The client's hotpatch revision; its reply, 243, is never sent | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §2, 3.3 |
| 496 | C→S | `PROTOC2G_GM_SCRIPTS` | No arguments, sent by a shipping client; its reply, 216, is never sent | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §2, 3.3 |
| 304 | C→S | `PROTOC2S_ROLE_CREATE` | One table: the new character's name, vocation, skeleton and appearance; answered with 628 or 270 | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §5 |
| 166 | C→S | `PROTOC2S_ROLE_DELETE` | The pid to delete; answered with 84 or 270 | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §7 |
| 10 | C→S | `PROTOC2S_ROLE_SELECT` | The pid to enter the world with; M3 answers with 270 | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §10 |
| 26 | C→S | `PROTOC2G_ROLE_RENAME` | Two values from the rename dialog | `named` | **Observed** (the sender) | [`packets/lobby.md`](packets/lobby.md) §2 |
| 454 | C→S | `PROTOC2G_ROLE_REBORN` | Three values from the reborn dialog | `named` | **Observed** (the sender) | [`packets/lobby.md`](packets/lobby.md) §2 |
| 321 | C→S | `PROTOC2G_ROLE_SLOT` | A pid, its new slot, and nil or the pid it swaps with; needs no reply | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §2, 4.2 |
| 392 | C→S | `PROTOC2S_CLIENT_EXIT` | No arguments: the client is quitting | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §2 |
| 228 | S→C | `PROTOG2C_ROLE_LIST` | The pid to pre-select, the list of actors, and the account's extras | `schema` | **Observed** (bytecode), **Inferred** (meaning) | [`packets/lobby.md`](packets/lobby.md) §3, §4 |
| 628 | S→C | `PROTOG2C_ROLE_CREATE` | The new actor | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §6 |
| 84 | S→C | `PROTOG2C_ROLE_DELETE` | The pid, a pending time in ms and a level threshold | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §7 |
| 270 | S→C | `PROTOS2C_ERROR_CODE` | An `errors` sheet `ID`, then format arguments | `schema` | **Observed** (bytecode) | [`packets/lobby.md`](packets/lobby.md) §8 |
| 495 | S→C | `PROTOG2C_SERVER_LIST` | Six values: the server and group lists, the server map, the homeland groups and two user ids. Not sent | `named` | **Observed** (the handler) | [`packets/lobby.md`](packets/lobby.md) §2 |
| 509 | S→C | `PROTOG2C_ROLE_SLOT` | Two values, only logged. Not sent | `named` | **Observed** (the handler) | [`packets/lobby.md`](packets/lobby.md) §2 |
| 368 | S→C | `PROTOS2C_PLATFORM_BILLING_REMAIN` | Six billing values. Not sent | `named` | **Observed** (the handler) | [`packets/lobby.md`](packets/lobby.md) §2 |
| 786 | S→C | `PROTOG2C_QUEUE_WAIT_TIME` | Two values for the login queue's display. Not sent | `named` | **Observed** (the handler) | [`packets/lobby.md`](packets/lobby.md) §2 |
| 243 | S→C | `PROTOG2C_HOTPATCH_INIT` | A revision and files that replace scripts and layouts. Never sent | `schema` | **Observed** (the name), **Inferred** (the effect) | [`packets/lobby.md`](packets/lobby.md) 2.3 |
| 216 | S→C | `PROTOG2C_GM_SCRIPTS` | Files that replace scripts and layouts. Never sent | `schema` | **Observed** (the name), **Inferred** (the effect) | [`packets/lobby.md`](packets/lobby.md) 2.3 |
| 556 | S→C | `PROTOS2C_GM_SCRIPT_INVOKE` | The server sends a chunk of Lua bytecode; the client runs it and answers with 278 (`PROTOC2S_GM_SCRIPT_REPORT`) and the chunk's MD5. Never sent | `schema` | **Observed** (the name), **Inferred** (the effect) | `data/core/client/net_manager.luac` `0.14`, `0.15`; [`packets/lobby.md`](packets/lobby.md) 2.3 |
| 627 | S→C | `PROTOG2C_SHEET_HOTPATCH` | Patches the client's content tables. Never sent | `named` | **Observed** (the name), **Inferred** (the effect) | [`packets/lobby.md`](packets/lobby.md) 2.3 |
| 729 | S→C | `PROTOG2C_USER_DETAIL` | Writes a file to the player's disk. Never sent | `named` | **Observed** (the name), **Inferred** (the effect) | [`packets/lobby.md`](packets/lobby.md) 2.3 |
| 720 | S→C | `PROTOS2C_CUSTOMER_FEEDBACK_URL` | A URL the client opens in the default browser. Never sent | `schema` | **Observed** (the name), **Inferred** (the effect) | [`packets/lobby.md`](packets/lobby.md) 2.3 |
| 598 | S→C | `PROTOS2C_GAMEFORGE_SEND_RECHARGE_URL` | A URL the client opens in the Gameforge overlay. Never sent | `schema` | **Observed** (the name), **Inferred** (the effect) | [`packets/lobby.md`](packets/lobby.md) 2.3 |

## REA Evidence IDs

Read through REA 6.3.0 with Ghidra 12.1.4 on the pinned exe, in one session on 2026-10-12, after the findings had been written from the shared Ghidra project; each reading shows the function in the role the finding gives it. An ID covers its call exactly as sent, so anyone can check it by repeating the call ([`../re/README.md`](../re/README.md#how-to-cite)). The handlers in the tables (`named`) are cited by address only: their role comes from FUN_140222E00's switch (O.1).

| Function | Cited in | Call | Evidence ID |
|---|---|---|---|
| FUN_1400D5850 | O.1 | `procedure_pseudo_code {procedure: "0x1400d5850"}` | `ev_af1c4fbf5d6aed889b7d529dd45f721617274b50f2625f323d3038aae2e57c64` |
| FUN_140221940 | O.2 | `procedure_pseudo_code {procedure: "0x140221940"}` | `ev_df70f99b7d9001ad43b2e331b1ce2368470ed55534010b42780506ecc795c047` |
| FUN_140221A00 | O.2 | `procedure_pseudo_code {procedure: "0x140221a00"}` | `ev_b88b93181e1a9ba5a8e782085daa916772b3fd53bebdefdfd36bcf0a682e4e7e` |
| FUN_140221AC0 | O.2 | `procedure_pseudo_code {procedure: "0x140221ac0"}` | `ev_7e98809795a3c282a3c1ee14214a407b4ac57c8f3191f598ebd6c1338832d2c0` |
| FUN_140221BB0 | O.2 | `procedure_pseudo_code {procedure: "0x140221bb0"}` | `ev_cecccd177df87001878f48ca00d2f3afac79247d8791f6627bb9c732374b329c` |
| FUN_140221CB0 | O.2 | `procedure_pseudo_code {procedure: "0x140221cb0"}` | `ev_7b2afb8ab324db3148928a4fe549ce4628cf49b458566e0a9be3939fdb81d3f6` |
| FUN_140221D30 | O.2 | `procedure_pseudo_code {procedure: "0x140221d30"}` | `ev_eba0244ecb8c94a02f7e0ad89c87cc89fc99e5e9a426d10e11f69e7a85bd3b30` |
| FUN_1402226B0 | O.2 | `procedure_pseudo_code {procedure: "0x1402226b0"}` | `ev_a2818646bcaa9860c1f5dd596f7ff052dfb3c66429aed40f8a50776a83cd13d2` |
| FUN_140222E00 | O.1 | `procedure_pseudo_code {procedure: "0x140222e00"}` | `ev_4656519e08dab96fa5ad38e228a578375ef1b30907fb075530e4d9e15b4d3d20` |
| FUN_1402234D0 | O.1 | `procedure_pseudo_code {procedure: "0x1402234d0"}` | `ev_d1ad9b7dfee67fc633ddfad3893990aa26d82423ba7721aae5003404d6605265` |
| FUN_14029A870 | O.1 | `procedure_pseudo_code {procedure: "0x14029a870"}` | `ev_4287eb1c9108d48b68771d0a270f2344b05732bb6c87fa64aff4e688271065a1` |
| FUN_14029AC60 | O.2, O.3 | `procedure_pseudo_code {procedure: "0x14029ac60"}` | `ev_000ec1319aee8193de7f61d599aff5a47f5ca4c72c89233c9892d8d247c99816` |
