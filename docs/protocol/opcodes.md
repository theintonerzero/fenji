# Opcodes

The inner message header's type byte ([`transport.md`](transport.md) §7) is the opcode. The client names its opcodes in two enums, `STC_PROTOCOL_*` (server to client) and `CTS_PROTOCOL_*` (client to server), listed in [spike-0](../re/spike-0.md) 4.12. This ledger gives each one's wire value. Most game traffic rides on the two script opcodes, `STC_PROTOCOL_SCRIPT` and `CTS_PROTOCOL_GATE_SCRIPT` / `CTS_PROTOCOL_SCRIPT`, whose bodies are pickled values: a script message id first, then its arguments ([`payloads.md`](payloads.md)).

**Method.** This ledger reads the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`) in the shared Ghidra project, read-only, together with the client's own scripts disassembled and decompiled locally by `luatool` (M1). Script evidence cites the script's engine path. It was written by a session that has not read any source rejected in [`../re/sources.md`](../re/sources.md). Statuses follow [`README.md`](README.md): `named` (the client's name is known, the payload isn't mapped), `schema` (payload mapped from the client's code), `verified` (seen against the real client in a sandboxed run and covered by a test).

## How the values were found

| # | Finding | Tag | Evidence |
|---|---|---|---|
| O.1 | **Server to client.** The network thread (FUN_1402234D0) hands every inbound message of type 2 or more to the scripts as an event, and closes the connection on type 1 (FUN_14029A870). For each event, FUN_140222E00 pushes the type, then decodes the body by type in a switch over 0 to 29: type 0 is unpickled (FUN_1400D5850), type 1 pushed as a string, type 2 updates the ping estimate, and types 3 to 29 each have their own parser. Those cases line up with the 30 `STC_PROTOCOL_*` names in their table order (the 31st, `MAX`, is the count). | **Inferred** | FUN_1402234D0, FUN_140222E00 |
| O.2 | **Client to server.** The NetClientLua methods post send commands to the network thread, which writes the command's type byte into the message header (FUN_14029AC60): `send_gateway` uses 0xFF, `send` 0, `send_time_sync` 2, `send_enter` 3, `send_leave` 4 and `send_sync_filter` 5. That is `CTS_PROTOCOL_GATE_SCRIPT` as −1 (0xFF as a byte) and the rest of the enum counting from 0 in table order. | **Inferred**; the type constants **Observed** | FUN_140221940, FUN_140221A00, FUN_140221AC0, FUN_140221BB0, FUN_140221CB0, FUN_140221D30, FUN_1402226B0 |
| O.3 | The client sends only in state 3 `WORK` ([`transport.md`](transport.md) 7.3), and the scripts' senders check that state themselves (`net_manager_send_gateway`, `net_manager_send`; `data/core/client/net_manager.luac`). | **Inferred** | FUN_14029AC60; net_manager.luac |

## Server to client (`STC_PROTOCOL_*`)

| Value | Direction | Client name | Handler | Status | Evidence |
|---|---|---|---|---|---|
| `0` | S→C | `STC_PROTOCOL_SCRIPT` | unpickled (FUN_1400D5850), then the scripts' handler for the message id (`data/core/client/net_manager.luac`) | `schema` | O.1, [`payloads.md`](payloads.md) |
| `1` | S→C | `STC_PROTOCOL_CLOSE` | FUN_14029A870: the body (at most 255 bytes kept) becomes the close reason, the socket closes, state 5 | `schema` | O.1, [`transport.md`](transport.md) 7.4 |
| `2` | S→C | `STC_PROTOCOL_TIME_SYNC` | FUN_140222E00 case 2: a `u32` and a `u64` from the body feed the ping estimate | `named` | O.1 |
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
| `0xFF` | C→S | `CTS_PROTOCOL_GATE_SCRIPT` | `send_gateway` (FUN_140221940): the pickled message id and arguments, for the gate | `schema` | O.2, [`payloads.md`](payloads.md) |
| `0` | C→S | `CTS_PROTOCOL_SCRIPT` | `send` (FUN_140221A00): the pickled message id and arguments | `schema` | O.2, [`payloads.md`](payloads.md) |
| `1` | C→S | `CTS_PROTOCOL_GM_CHANNEL` | — | `named` | O.2 |
| `2` | C→S | `CTS_PROTOCOL_TIME_SYNC` | `send_time_sync` (FUN_140221AC0): a 13-byte body | `named` | O.2 |
| `3` | C→S | `CTS_PROTOCOL_CLIENT_ENTER` | `send_enter` (FUN_140221BB0): a 21-byte body | `named` | O.2 |
| `4` | C→S | `CTS_PROTOCOL_CLIENT_LEAVE` | `send_leave` (FUN_140221CB0): an empty body | `schema` | O.2 |
| `5` | C→S | `CTS_PROTOCOL_SYNC_FILTER_SET` | `send_sync_filter` (FUN_140221D30): an 18-byte body | `named` | O.2 |
| `6`–`11` | C→S | `CTS_PROTOCOL_QUERY_ACTOR_POS`, `_QUERY_ACTOR_MOVE_CMD`, `_MOVE_DIR`, `_MOVE_COMBAT`, `_MOVE_FINISH_CHARGE`, `_MOVE_DEBUG` | by table order; their senders weren't read | `named` | O.2 |

The type-0 message the client sends during auth ([`transport.md`](transport.md) §6) isn't a script message: the client builds it directly in the hello handler, before state `WORK`.

## Script messages

Script message ids are numbers that the scripts' protocol enum assigns at load time (`data/core/share/enum_protocols.luac` sets each `PROTOG2C_*`, `PROTOC2G_*`, `PROTOS2C_*` and `PROTOC2S_*` name from a global, not a literal), so the names don't follow their order in that file. The ids used so far:

| Id | Direction | Name | What it does | Tag | Evidence |
|---|---|---|---|---|---|
| 414 | S→C, `STC_PROTOCOL_SCRIPT` | `PROTOG2C_SET_NET_CONNECTED` (the handler logs this name) | Moves the client to `WORK`: it stores billing fields, calls `set_working`, fires `EVENT_NET_CONNECTED` and sends 402 ([`transport.md`](transport.md) §10) | **Inferred** | `data/core/modules/net/net.luac` (the handler at id 414) |
| 402 | C→S, `CTS_PROTOCOL_GATE_SCRIPT` | Unknown | Sent with no arguments right after 414 | **Inferred**; the name **Unknown** | net.luac |
| 556 | S→C | — | The server sends a chunk of Lua bytecode; the client runs it and answers with 278 and the chunk's MD5. Not needed by Fenji | **Inferred** | `data/core/client/net_manager.luac` |
