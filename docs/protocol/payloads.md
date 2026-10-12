# Payloads

The client serialises script messages and its login data with its own "pickle" format, `ks.pickle_pack` and its native counterpart `kslua_pickle_pack`. It is neither Python's pickle nor msgpack, which answers [spike-0](../re/spike-0.md) 4.9 for the script opcodes ([`opcodes.md`](opcodes.md)). A packet is a count of top-level values, then the values, each a type tag followed by its data, little-endian.

**Method.** This reads the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`) in the shared Ghidra project, read-only: the unpacker FUN_1400D5850, which calls FUN_1400D56D0 for the header (with FUN_1400D55C0 and FUN_1400CF400 for a compressed packet) and FUN_1400D4AC0 for each value, the packers' shared last step, FUN_1400D53D0, and the packing itself (FUN_1400D5280, FUN_1400D41A0 and FUN_1400D3950; P.3, P.7, P.8). Each function cited here was also read through REA, after the evidence norm ([`../re/README.md`](../re/README.md#the-norm)); the Evidence IDs are in [REA Evidence IDs](#rea-evidence-ids). That reading was then checked against real client output: the auth payloads of runs m2-r1 and m2-r5 ([`transport.md`](transport.md) §9) decode with it to the last byte (P.4, P.6). Written by a session that has not read any source rejected in [`../re/sources.md`](../re/sources.md). Tags as in [spike-0](../re/spike-0.md).

## The format

| # | Finding | Tag | Evidence |
|---|---|---|---|
| P.1 | **Header.** An empty packet holds no values. A leading `FF` byte means the rest is compressed (FUN_1400D55C0): 3 bytes, a big-endian 24-bit raw size of at most 0x100000 ("pkt bad size, too large zip size"), then an LZ4 block. FUN_1400CF400, an LZ4 block decoder shaped like the reference library's `LZ4_decompress_safe`, decodes it into at most the raw size and returns the length it wrote ("pkt unzip failed!" otherwise). The count byte and the values are then read from those bytes, with no second `FF`. The count byte `c`: if `c < 0x80`, `c` values follow. Otherwise `c & 0x7F` values follow after 3 more bytes, a big-endian 24-bit count of tables that later values may refer back to. The values must end exactly where the packet, or its decompressed bytes, end ("pkt length not match"). | **Inferred**; error strings **Observed** | FUN_1400D56D0, FUN_1400D55C0, FUN_1400CF400 |
| P.2 | **Values.** One type tag byte, then (table below). Integers become Lua integers and `07` a Lua float. A table is a `u32` array count, that many values for indices 1, 2, …, then key/value pairs ended by a `nil` key. Tables nest at most 31 deep: FUN_1400D5850 starts the unpacker's depth count at 1 (its constant at 0x142F04970), each table adds 1, and a table that takes the count past 32 is refused ("unpack table too depth!"). Until 2026-10-12 this row said 32, having missed the start value. | **Inferred**; error strings and the constant **Observed** | FUN_1400D4AC0, FUN_1400D5850 |
| P.3 | **What the client's packers write.** `ks.pickle_pack` (FUN_1400D5AC0) and `kslua_pickle_pack` (FUN_1400D54F0) both start by emptying the packer's record of the tables it has packed (FUN_14013B2F0, "attempt to reset a locked table!") and resetting its state, its depth count to 1 (FUN_1400D33F0). FUN_1400D5280 then packs at most 32 values ("arg count MUST <= %d") and writes the header: the count byte if it packed no table, otherwise `0x80` OR the count, then the number of tables it packed as a 24-bit big-endian count (P.1). FUN_1400D41A0 picks each value's tag by its Lua type: `01` or `02` for a boolean; for a light userdata, `0C` if it is null and `00` otherwise; for an integer, `03` to `06` by its absolute value (below 0x80, 0x8000 and 0x80000000), so −128 takes `04`; `07` for any other number; `08` to `0A` for a string, by its length; P.7 for a table and P.8 for a full userdata; `00` for anything else. The decoder takes any integer width for any value, so an encoder may pick the shortest. | **Inferred**; error strings **Observed** | FUN_1400D5AC0, FUN_1400D54F0, FUN_14013B2F0, FUN_1400D33F0, FUN_1400D5280, FUN_1400D41A0, FUN_1400D4AC0 |
| P.4 | **Checked against the client.** The 73-byte auth payload of run m2-r1 decodes with P.1–P.2 to exactly 10 values and ends on its last byte (the auth payload, below). | **Observed** | m2-r1 (`re/stub-logs/m2-r1/*.auth.bin`) |
| P.5 | **When the client compresses.** Both packers, `ks.pickle_pack` (FUN_1400D5AC0) and `kslua_pickle_pack` (FUN_1400D54F0), end in FUN_1400D53D0. It compresses a packet of at least the zip limit with LZ4 (FUN_1400CF3B0), and keeps the result when it plus the 4-byte header is shorter than the packet: `FF`, the exact raw size, the block. The limit, DAT_1431C84F8, is 0x80 bytes. `ks.pickle_zip_limit` (FUN_1400D5A20) returns it and takes a new value only above 0x7F, and no script the client ships calls it. So a client message of 128 bytes or more usually arrives compressed. | **Inferred** | FUN_1400D53D0, FUN_1400D5A20, FUN_1400D7660 |
| P.6 | **Checked against the client, compressed.** The 285-byte auth payload of run m2-r5 starts `FF 00 01 60`: a 352-byte packet in a 281-byte LZ4 block. Decompressed, it decodes with P.1–P.2 to exactly 10 values (below). `fenji_proto::pickle::decode` reads it too (an ignored test, run against the local log). | **Observed** | m2-r5 (`re/stub-logs/m2-r5/*.auth.bin`); `crates/fenji-proto/tests/fixtures.rs` (`r5_compressed_auth_payload_decodes`) |
| P.7 | **Table references in the client's messages.** FUN_1400D3950 numbers each new table it packs from 0, in the order it reaches their `0B` tags, and writes a table it has already packed in the same packet as `0E`, `0F` or `10` with that number, the shortest form that fits. A new table is `0B`, its array length (at most 0x7FFFFFFF, "too large table array!"), the array values, the other pairs and a `nil` key. Each table adds 1 to the depth count, and the packer refuses one that takes it past 32 ("pack table too depth!"): a client sends at most 31 nested tables, as many as its unpacker takes (P.2). So every client message that carries a table has the `0x80` header (P.3), and the unpacker numbers its tables the same way ([`../re/m2-tables.md`](../re/m2-tables.md) 2.2). The messages seen so far, the auth payload and `402`, carry none. | **Inferred**; error strings **Observed** | FUN_1400D3950, FUN_1400D4AC0 |
| P.8 | **Magic objects in the client's messages.** For a full userdata, FUN_1400D41A0 looks for a `__pickle__` function in its metatable. Without one, it writes `00`. With one, it calls the function on the userdata and writes `0D`, then the function's two results as values: the name of the constructor that rebuilds the object and its argument, which the unpacker passes to that constructor ([`../re/m2-tables.md`](../re/m2-tables.md) 2.4). A magic object adds nothing to the depth count, packing or unpacking. Which userdata the client's messages carry wasn't traced. | **Inferred**; which userdata **Unknown** | FUN_1400D41A0, FUN_1400D4AC0 |

| Tag | Value | Data after the tag |
|---|---|---|
| `00` | `nil` | none |
| `01` | `true` | none |
| `02` | `false` | none |
| `03` | integer | `i8` |
| `04` | integer | `i16` |
| `05` | integer | `i32` |
| `06` | integer | `i64` |
| `07` | number | `f64` |
| `08` | string | `u8` length, then the bytes |
| `09` | string | `u16` length, then the bytes |
| `0A` | string | `u32` length, then the bytes |
| `0B` | table | `u32` array count, the array values, then key/value pairs ended by a `nil` key |
| `0C` | a null light userdata | none |
| `0D` | an object rebuilt by a registered constructor ("magic", P.8) | the constructor's name, then its argument, each a value |
| `0E`, `0F`, `10` | a reference to an earlier table (P.7) | `u8`, `u16` or `u32` index |

Any other tag is refused ("pkt bad type(%d)"). `fenji_proto::pickle` reads every tag, compressed or not, up to 65,536 values a packet (#84). It writes every tag, uncompressed, with the header's table count only when a reference needs it.

## The auth payload

The payload of the client's auth message ([`transport.md`](transport.md) §6) is `ks.pickle_pack` of ten values, in the order `net_manager_login` (`data/core/client/net_manager.luac`) passes them. `NET.Login` (`data/core/modules/net/net.luac`) picks the first five for each login path. On the server-list path the packet is over 128 bytes, so it arrives compressed (P.5, P.6).

| # | m2-r1 (launcher path) | m2-r5 (server-list path, mocked SDK) | Meaning | Tag |
|---|---|---|---|---|
| 1 | `"fenji-dev"` | a 282-character hex string: a JSON object with the keys `account_display_name`, `auth_ticket`, `auth_type` (`"mocked"`) and `external_account_id` | the ticket: `gj_ticket` on the launcher path, `net_manager_gameforge_get_ticket()` on the Gameforge path | **Observed** (values); meaning **Inferred** |
| 2 | `0` | `0` | a constant: `NET.Login` passes 0 on every login path; the account type it reads from `gj_uinfo` isn't sent | **Observed** (values); meaning **Unknown** |
| 3 | `"tester"` | `"account-name"` | the account: `gj_uinfo`'s fifth field, or `net_manager_gameforge_get_account_name()` | as row 1 |
| 4 | `1` | `nil` | the user id: `gj_uinfo`'s sixth field; the Gameforge path leaves it `nil` | as row 1 |
| 5 | `1` | `0` | `gj_uinfo`'s first field, which [`login.md`](login.md) 2.8 calls the server id; elsewhere a per-platform constant: 0 Gameforge, 99 Steam, 403000 or 403100 WeGame | **Observed** (values); meaning **Unknown** |
| 6 | `"2.1.19.42851"` | `"2.1.19.42851"` | the client version (`app_get_version`) | as row 1 |
| 7 | `""` | `""` | a script-side value (unnamed) | **Observed** (values); meaning **Unknown** |
| 8 | `0` | `0` | the result of a local file check (a `pcall`) | **Observed** (values); meaning **Unknown** |
| 9 | a 20-byte string | a 20-byte string | `gm_client_get_mfp()`: a machine fingerprint. Kept out of every fixture | **Observed** (length); meaning **Inferred** |
| 10 | `"en-GB"` | `"de-DE"` | `gameforge_get_user_language()` | **Observed** (values); meaning **Inferred** |

## Script messages

A script message's body is a pickle whose first value is the message id, followed by the handler's arguments ([`opcodes.md`](opcodes.md), Script messages). For example, `PROTOG2C_SET_NET_CONNECTED` (414) is handled with three arguments: a time-zone offset (ignored when `nil`), a table of billing fields (`times_remain`, `coins_remain`, `bind_coins_remain`, `cards_endtime`; it must be a table, and missing fields are `nil`), and a user id (`nil` keeps the current one) (**Inferred** from the handler, [`transport.md`](transport.md) 10.3). An empty table, the body `04 04 9E 01 00 0B 00 00 00 00 00 00`, is enough for the handler, but a listener of `EVENT_NET_CONNECTED` then formats a missing billing number and fails (run m2-r4). So Fenji sends the four fields as 0, an 82-byte body from `fenji_proto::packet::set_net_connected`: `04 04 9E 01 00 0B 00 00 00 00`, then for each field `08`, its name's length and the name, then `03 00`, in the order above, and last `00 00`, the table's end and the `nil` user id. That this body moves the client to `WORK` cleanly is **Observed** in runs m2-r4b and m2-r5 ([`transport.md`](transport.md) 10.4).

## REA Evidence IDs

Read through REA 6.3.0 with Ghidra 12.1.4 on the pinned exe, in two sessions on 2026-10-12 (the second for P.2's depth count, P.3, P.7 and P.8), after the findings had been written from the shared Ghidra project; each reading shows the function in the role the finding gives it. An ID covers its call exactly as sent, so anyone can check it by repeating the call ([`../re/README.md`](../re/README.md#how-to-cite)). DAT_1431C84F8 is data, which REA's procedure queries don't cover; its value 0x80 comes from the shared Ghidra project (P.5). So do the 16 bytes at 0x142F04970, `FE FF FF FF 00 00 00 00 01 00 00 00 00 00 00 00`, which FUN_1400D5850 copies into the unpacker's state: the 1 at byte 8 is where its depth count starts (P.2). REA's pseudocode of FUN_1400D5850 shows the same 1.

| Function | Cited in | Call | Evidence ID |
|---|---|---|---|
| FUN_1400CF3B0 | P.5 | `procedure_pseudo_code {procedure: "0x1400cf3b0"}` | `ev_f77a2e094179f3494b3f015d1cd09abd95b849e6715b01c9e846d19fcdb3850e` |
| FUN_1400CF400 | Method, P.1 | `procedure_pseudo_code {procedure: "0x1400cf400"}` | `ev_8877b1152ea9a224274169d1d1f31f03e771e4a33dd10ce37937fc0a8e8d65d2` |
| FUN_1400D33F0 | P.3 | `procedure_pseudo_code {procedure: "0x1400d33f0"}` | `ev_b146cd1d43ce4c68a3894d31dd1b60e4017bfe515c52a1490f69d0c5b9cb673c` |
| FUN_1400D3950 | P.7 | `procedure_pseudo_code {procedure: "0x1400d3950"}` | `ev_bc83076e377d0b202741c740b9e46777493dd20a003324b54549d01294bea68f` |
| FUN_1400D41A0 | P.3, P.8 | `procedure_pseudo_code {procedure: "0x1400d41a0"}` | `ev_52c0ec8942d43de50ba5345f019676dd8b758dfaf10f70086032cf3a7c079d40` |
| FUN_1400D4AC0 | Method, P.2, P.3, P.7, P.8 | `procedure_pseudo_code {procedure: "0x1400d4ac0"}` | `ev_1e76a59eba406f609264ca691ad3bccb2745968beb2eee7d29ff0d7553d89644` |
| FUN_1400D5280 | Method, P.3 | `procedure_pseudo_code {procedure: "0x1400d5280"}` | `ev_cb34a8a1d02b59013d2bc2558c3f7991686e0c612d9720eb2471f41d84a24888` |
| FUN_1400D53D0 | Method, P.5 | `procedure_pseudo_code {procedure: "0x1400d53d0"}` | `ev_6a81a461fcb184bf11b2740aea8c26a299321f2d5ee976f4d08c3b75eeae3856` |
| FUN_1400D54F0 | P.3, P.5 | `procedure_pseudo_code {procedure: "0x1400d54f0"}` | `ev_72d7da265dc773a97706666c71deea2e6186b049b477a6ffccb8e4432d1d7fef` |
| FUN_1400D55C0 | Method, P.1 | `procedure_pseudo_code {procedure: "0x1400d55c0"}` | `ev_d9bfdb8a2f62393b0f94a71d356429c816cb025d2aa8325257026232680852f8` |
| FUN_1400D56D0 | Method, P.1 | `procedure_pseudo_code {procedure: "0x1400d56d0"}` | `ev_f590e6bd21bf27155144c8121d799eb9f37779edb6c824dbe6e452a187e235c4` |
| FUN_1400D5850 | Method, P.2 | `procedure_pseudo_code {procedure: "0x1400d5850"}` | `ev_af1c4fbf5d6aed889b7d529dd45f721617274b50f2625f323d3038aae2e57c64` |
| FUN_1400D5A20 | P.5 | `procedure_pseudo_code {procedure: "0x1400d5a20"}` | `ev_ce755cfc907afa564d1bee08d91e378251420ca36497ca01480a529629f9eda9` |
| FUN_1400D5AC0 | P.3, P.5 | `procedure_pseudo_code {procedure: "0x1400d5ac0"}` | `ev_93756521661ad1298dc666abd0f4d5657cca8242afe7d343dd9e2a8c288384f1` |
| FUN_1400D7660 | P.5 | `procedure_pseudo_code {procedure: "0x1400d7660"}` | `ev_892872725873074678d85f177a260fd4f7bf23dcdbccaf4ad4893f3504196f0e` |
| FUN_14013B2F0 | P.3 | `procedure_pseudo_code {procedure: "0x14013b2f0"}` | `ev_224d1178fbbc4efe911fa9d66b9b3291bb70183e4d454040402d419e9f8e63ed` |
