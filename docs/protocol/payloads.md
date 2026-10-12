# Payloads

The client serialises script messages and its login data with its own "pickle" format, `ks.pickle_pack` and its native counterpart `kslua_pickle_pack`. It is neither Python's pickle nor msgpack, which answers [spike-0](../re/spike-0.md) 4.9 for the script opcodes ([`opcodes.md`](opcodes.md)). A packet is a count of top-level values, then the values, each a type tag followed by its data, little-endian.

**Method.** This reads the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`) in the shared Ghidra project, read-only: the unpacker FUN_1400D5850, which calls FUN_1400D56D0 for the header and FUN_1400D4AC0 for each value. That reading was then checked against real client output: the auth payload of run m2-r1 ([`transport.md`](transport.md) §9) decodes with it to the last byte (P.4). Written by a session that has not read any source rejected in [`../re/sources.md`](../re/sources.md). Tags as in [spike-0](../re/spike-0.md).

## The format

| # | Finding | Tag | Evidence |
|---|---|---|---|
| P.1 | **Header.** An optional leading `FF` byte starts an extension section (FUN_1400D55C0, not read). Then one count byte `c`. If `c < 0x80`, `c` values follow. Otherwise `c & 0x7F` values follow after 3 more bytes, a big-endian 24-bit count of tables that later values may refer back to. The values must end exactly where the packet ends ("pkt length not match"). | **Inferred**; error strings **Observed** | FUN_1400D56D0 |
| P.2 | **Values.** One type tag byte, then (table below). Integers become Lua integers and `07` a Lua float. A table is a `u32` array count, that many values for indices 1, 2, …, then key/value pairs ended by a `nil` key; tables nest at most 32 deep. | **Inferred**; error strings **Observed** | FUN_1400D4AC0 |
| P.3 | **What the client accepts is wider than what it needs to.** The decoder takes any integer width for any value, so an encoder may pick the shortest. Which forms `kslua_pickle_pack` (FUN_1400D54F0) itself emits wasn't read. | **Inferred**; the packer's choices **Unknown** | FUN_1400D4AC0, FUN_1400D54F0 |
| P.4 | **Checked against the client.** The 73-byte auth payload of run m2-r1 decodes with P.1–P.2 to exactly 10 values and ends on its last byte (the auth payload, below). | **Observed** | m2-r1 (`re/stub-logs/m2-r1/*.auth.bin`) |

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
| `0D` | an object rebuilt by a registered constructor ("magic") | a value, then the constructor's arguments |
| `0E`, `0F`, `10` | a reference to an earlier table | `u8`, `u16` or `u32` index |

Any other tag is refused ("pkt bad type(%d)"). Fenji's encoder needs only `00` to `0B`.

## The auth payload

The payload of the client's auth message ([`transport.md`](transport.md) §6) is `ks.pickle_pack` of ten values, in the order `net_manager_login` (`data/core/client/net_manager.luac`) passes them:

| # | Value in m2-r1 | Meaning | Tag |
|---|---|---|---|
| 1 | `"fenji-dev"` | the ticket: `gj_ticket` on the launcher path | **Observed** (value); meaning **Inferred** |
| 2 | `0` | a constant: `NET.Login` (`data/core/modules/net/net.luac`) passes 0 on every login path; the account type it reads from `gj_uinfo` isn't sent | **Observed** (value); meaning **Unknown** |
| 3 | `"tester"` | account (`gj_uinfo`) | as row 1 |
| 4 | `1` | user id (`gj_uinfo`) | as row 1 |
| 5 | `1` | server id | as row 1 |
| 6 | `"2.1.19.42851"` | the client version (`app_get_version`) | as row 1 |
| 7 | `""` | a script-side value (unnamed) | **Observed** (value); meaning **Unknown** |
| 8 | `0` | the result of a local file check (a `pcall`) | **Observed** (value); meaning **Unknown** |
| 9 | a 20-byte string | `gm_client_get_mfp()`: a machine fingerprint. Kept out of every fixture | **Observed** (length); meaning **Inferred** |
| 10 | `"en-GB"` | `gameforge_get_user_language()` | **Observed** (value); meaning **Inferred** |

## Script messages

A script message's body is a pickle whose first value is the message id, followed by the handler's arguments ([`opcodes.md`](opcodes.md), Script messages). For example, `PROTOG2C_SET_NET_CONNECTED` (414) is handled with three arguments: a time-zone offset (ignored when `nil`), a table of billing fields (`times_remain`, `coins_remain`, `bind_coins_remain`, `cards_endtime`; it must be a table, and missing fields are `nil`), and a user id (`nil` keeps the current one). So its smallest body is `04 04 9E 01 00 0B 00 00 00 00 00 00`: four values, the id 414 as `i16`, `nil`, an empty table, `nil` (**Inferred**; [`transport.md`](transport.md) §10).
