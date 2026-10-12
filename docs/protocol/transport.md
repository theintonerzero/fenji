# Transport

The client opens one TCP connection to the gate and waits for the server to speak first: 64 bytes of hello, which it checks against a constant `K` that its own scripts pass in. Two words of the hello then pick one of 27 cipher variants for each direction, keyed from the same 64 bytes; Fenji uses case 3, AES-128, both ways. After the hello every byte travels in frames whose 16-bit length is XORed with a key that steps after each frame, and the server's frame bodies are Oodle Network data behind a raw-length prefix, while the client's are not. The client's first message carries a dictionary digest, a MAC address and its login payload, and a 2-byte `ok` reply moves it to `QUEUE`. Sandboxed runs against our stub (§9) confirmed the hello, the variant, the client's frames and keys and the auth message's layout. They also showed the client's decoder accepting a stored packet, from which we infer that the server needs no Oodle Network encoder.

**Method.** This reading comes from the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`, checked with `sha256sum` against the file the Ghidra project was imported from). It used read-only queries to the shared Ghidra project: decompiler output, listings, cross-references, strings and memory reads. For §1–§8 nothing was executed: the client was not run, and no script or debugger touched the project; §9 records the sandboxed runs that checked them afterwards (M2 plan Task 8). Two small programs of our own checked the arithmetic (L2). This document re-verifies the M2 plan's planning findings F1–F8; all eight agree with it, except that F2's key fold holds only for keys of up to 64 bytes (3.5). Every part was written by an agent that has not read any source rejected in [`../re/sources.md`](../re/sources.md). Tags follow [`../re/spike-0.md`](../re/spike-0.md): **Observed** (read from the pinned build by a tool: strings, bytes, constants in the code), **Inferred** (our reading, including code readings), **Unknown**. Addresses are virtual addresses in the pinned exe (image base 0x140000000), and evidence keys resolve in the [Evidence index](#evidence-index). This document predates the evidence norm settled in #18 ([`../re/README.md`](../re/README.md#evidence-for-gujianolexe)), so it cites pinned-exe addresses, which stay acceptable.

## 1. Connection and states

| Value | `KS_NET_CLIENT_ST_*` | What a poll (FUN_14029ADD0) does in this state |
|---|---|---|
| 0 | `INIT` | With no socket, tries the next gate address (1.2); with none left, sets 4. Once connected, waits for the hello (§2). |
| 1 | `IN_AUTH` | Sends a time sync when one is due (4.6), moves socket data, reads frames (§4, §5) and judges the first inbound message (6.6). If the socket fails during the poll, sets 4. |
| 2 | `QUEUE` | Time sync, socket, reads frames, frames queued outbound data (4.5). If the socket fails, sets 4. |
| 3 | `WORK` | The same as 2. Only in this state does the client queue new outbound messages (7.3). |
| 4 | `ERROR` | Nothing. |
| 5 | `CLOSED` | Nothing. |
| 6 | `CLOSED_VERSION` | Nothing; a failed hello check sets it (2.4). |
| 7 | `CLOSED_AUTH` | Nothing; a rejected auth sets it (6.6). |

The names are **Observed** strings. The values and the behaviour are **Inferred**: FUN_140222740 registers the names with the values 0–7 in this order, and the poll handles them as the table says (T1, T3).

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 1.1 | Scripts create the client with `net_client_create(gate, payload, K)` (FUN_140222970). FUN_14029A900 stores the low 32 bits of `K` at ctx+0x50 and a copy of the payload (at most 0x4000 bytes), and allocates a 0x40000-byte receive buffer and a 0x10000-byte send buffer. The gate string is a comma-separated list of up to 32 entries of 5 to 31 characters, each `a.b.c.d:port`, parsed with `inet_addr`, so a gate must be a dotted IPv4 address (FUN_140299940, FUN_1400C4200). | **Inferred** | T2 |
| 1.2 | Connecting: a non-blocking IPv4 TCP socket with `TCP_NODELAY`, and 20 seconds for each address (FUN_1400C4370; FUN_14029C030 passes 20). A failed or timed-out connect moves on to the next address, and after the last one the state is 4. The client sends nothing until the hello (spike-0 4.22; observed in [`login.md`](login.md) §6). | **Inferred** | T3 |
| 1.3 | A network thread (FUN_1402234D0) polls the client between waits on its command queue. It posts each change in the poll's result to the scripts, and in states 2 and 3 it hands them each inbound message (7.4). `net_client_dispatch` (FUN_140222AD0) delivers both. For a result of 4 or above, it returns the state with the close reason (ctx+0x270, FUN_14029A8E0) and the error code (ctx+0x14, FUN_14029A8D0), and frees the client (FUN_14029AAB0). The thread then stops polling it. | **Inferred** | T4 |
| 1.4 | Once its socket is closed, every later poll returns 5 at once, whatever state is stored. So the two errors that close the socket without setting a state, a failed Oodle decode (0x5F5E101) and a failed append (0x5F5E102), reach the scripts as state 5. A close message from the server sets 5 itself (7.4). A socket that fails during a poll in states 1–3 gives state 4, because the poll then finds the connection flag (ctx+0x10) cleared. | **Inferred** | T3 |
| 1.5 | An orderly close by the server, a zero-byte read, isn't an error for the socket code (FUN_1400C44D0): the client notices only when a later send or read fails. | **Inferred** | T3 |
| 1.6 | The error code at ctx+0x14 shares its field with the socket layer's last Winsock error, so a socket failure leaves a Winsock code there (FUN_1400C44D0, FUN_1400C4370). | **Inferred** | T3 |

The client's own error codes run from 0x5F5E101 to 0x5F5E107 (100,000,001 to 100,000,007). A whole-program search of instruction constants found the five below; 0x5F5E103 and 0x5F5E105 appear nowhere in the exe, as instruction constants or as bytes (L1):

| Code | Set at | When | State afterwards | Tag |
|---|---|---|---|---|
| 0x5F5E101 | 0x14029B9B4, FUN_14029B850 | `OodleNetwork1TCP_Decode` fails on a server frame (5.3) | Socket closed; stored state unchanged; reported as 5 (1.4) | **Inferred**; constant **Observed** |
| 0x5F5E102 | 0x14029B9A2, FUN_14029B850 | Appending decoded bytes to the inbound stream fails (out of memory) | As 0x5F5E101 | **Inferred**; constant **Observed** |
| 0x5F5E103 | nowhere | Unused in this build | — | **Observed** (absent) |
| 0x5F5E104 | 0x14029C3FB, FUN_14029C330 | The hello check fails (2.4) | 6 `CLOSED_VERSION` | **Inferred**; constant **Observed** |
| 0x5F5E105 | nowhere | Unused in this build | — | **Observed** (absent) |
| 0x5F5E106 | 0x14029AFC4, FUN_14029ADD0 | The first message in `IN_AUTH` isn't `ok` (6.6) | 7 `CLOSED_AUTH` | **Inferred**; constant **Observed** |
| 0x5F5E107 | 0x14029A8D9, FUN_14029A8D0 | The error-code getter is called with no client | — | **Inferred**; constant **Observed** |

## 2. The hello

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | The server speaks first with 64 bytes, outside any frame. The client waits until 64 bytes are buffered (0x40 at 0x14029C38F) and reads them as 16 little-endian `u32` words `w0`…`w15`. | **Inferred**; constant **Observed** | T5 |
| 2.2 | The check: `w[(w1 mod 14) + 2] == (K OR 0xC0000000) XOR w1 XOR w0`. The index is a division by 14 in the code (0x24924925 and its shifts, then times 14 and plus 2, at 0x14029C3C3–0x14029C3E8); our exhaustive check over all 2^32 values of `w1` confirms it equals `(w1 mod 14) + 2` (L2). The check word is one of `w2`…`w15`, never `w0` or `w1`. | **Inferred**; constants **Observed** | T5, L2 |
| 2.3 | `K` is ctx+0x50, the third argument of `net_client_create` (read with the Lua integer check, low 32 bits kept). The scripts pass 3861871408 = 0xE62F7B30 ([`login.md`](login.md) 2.6), whose top two bits are already set, so the OR changes nothing. No key or signature is involved: any server can build a hello the client accepts. | **Inferred** | T2, T5 |
| 2.4 | A mismatch sets error 0x5F5E104, closes the socket and sets state 6, `CLOSED_VERSION` (0x14029C3FB, 0x14029C407). So `K` works as a protocol version, which answers part of [`login.md`](login.md) 2.7. | **Inferred** | T5 |
| 2.5 | A match: `w0 mod 27` picks the variant of the context at ctx+0x3C0, which the client uses to receive (server → client), and `w1 mod 27` the one at ctx+0x1488, which it uses to send (client → server). Each variant's init then reads all 64 bytes (§3). The client consumes the 64 bytes and keeps any later bytes for the frame reader (§4), sets state 1 `IN_AUTH` (0x14029C498), and queues and sends its auth (§6) in the same poll. | **Inferred** | T5, T6 |
| 2.6 | Example: `w0 = w1 = 3` picks case 3 both ways. The check index is `(3 mod 14) + 2 = 5`, so `w5 = 0xE62F7B30 XOR 3 XOR 3 = 0xE62F7B30`. The other 13 words are free. | **Inferred** (computed) | L2 |

## 3. Variants

FUN_14029BA10 clears a 0x10C8-byte context and switches on `w mod 27`. Every case fills the same four slots:
- `[0]`: FUN_14008E470, an empty function that thousands of other sites share;
- `[1]`: the init, `init(context, hello, 64)`, which folds the hello into a key (3.5) and runs the cipher's key set-up into context+0x20;
- `[2]`: the send transform, called on each outbound data frame (context at ctx+0x1488);
- `[3]`: the receive transform, called on each inbound data frame (context at ctx+0x3C0).

The cases form eight families. Each family shares its two transforms, and each family's set-up, encrypt and decrypt functions are the ones that reference its LibTomCrypt source-file string.

| Family (source string) | Cases: key bytes | Block | Tail constant | Set-up / encrypt / decrypt | Send `[2]` / receive `[3]` |
|---|---|---|---|---|---|
| AES (`src\ciphers\aes\aes.c`) | 3: 16, 10: 32, 19: 24 | 16 | 0xC6 | FUN_14008EBC0 / FUN_14008F3B0 / FUN_14008FA10 | FUN_140299B90 / FUN_140299AD0 |
| Blowfish (`src\ciphers\blowfish.c`) | 2: 32, 6: 8, 22: 56 | 8 | 0x29 | FUN_14008D5B0 / FUN_14008D860 / FUN_14008DB00 | FUN_14029A250 / FUN_14029A190 |
| RC5 (`src\ciphers\rc5.c`) | 1: 8, 5: 32, 9: 128, 13: 96 | 8 | 0x28 | FUN_14008DDA0 / FUN_14008DFD0 / FUN_14008E160 | FUN_14029B420 / FUN_14029B360 |
| RC6 (`src\ciphers\rc6.c`) | 4: 128, 11: 8, 17: 64, 21: 16 | 16 | 0xB1 | FUN_14008E4F0 / FUN_14008E780 / FUN_14008E9A0 | FUN_14029B730 / FUN_14029B670 |
| Twofish (`src\ciphers\twofish\twofish.c`) | 0: 24, 20: 16, 23: 32 | 16 | 0x26 | FUN_140090930 / FUN_140090ED0 / FUN_140091160 | FUN_14029C210 / FUN_14029C150 |
| CAST5 (`src\ciphers\cast5.c`) | 7: 16, 15: 5 | 8 | 0x13 | FUN_1400913E0 / FUN_1400920E0 / FUN_140092750 | FUN_14029A490 / FUN_14029A3D0 |
| Anubis (`src\ciphers\anubis.c`) | 8: 36, 12: 40, 14: 28, 16: 32, 18: 16, 24: 20, 26: 24 | 16 | 0xAE | FUN_140092DC0 / FUN_140093780 / FUN_140093880 | FUN_140299FB0 / FUN_140299EF0 |
| XTEA (`src\ciphers\xtea.c`) | 25, the `default` branch: 16 | 8 | 0x37 | FUN_1400900C0 / FUN_140090200 / FUN_1400903C0 | FUN_14029C820 / FUN_14029C760 |

The strings, the key lengths and the tail constants are **Observed**; the families, block sizes and roles are **Inferred** (T6, T8, T9). Fenji implements only case 3; the others are listed so that a different choice later starts from this table.

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 3.1 | The eight source-file strings sit at 0x142EEC2F0 and 0x142EFC368–0x142EFC400. Each is referenced by its family's set-up, encrypt and decrypt functions, which name it in their argument-check messages. | **Observed** | T9 |
| 3.2 | The AES functions are standard AES. The set-up (FUN_14008EBC0) accepts 16-, 24- or 32-byte keys, loads them big-endian and expands them with the standard tables and round constants: the table at 0x142EF0060 starts `63 7C 77 7B`, the AES S-box, and the round constants are 01 (inline in the code) and then, as the top byte of each word from 0x142EEEC34, `02 04 08 10 20 40 80 1B 36`. The encrypt function's first table (0x142EEF860) starts with 0xC66363A5 and 0xF87C7C84. The inits pass 0 rounds, which means the default: 10 for a 16-byte key. So case 3 is FIPS-197 AES-128, one block at a time. | **Inferred**; table bytes **Observed** | T8 |
| 3.3 | Case 3's init (FUN_140299C50) passes 16 both to the fold (0x140299C73) and to the AES set-up (0x140299C84): AES-128. Case 10 (FUN_140299A70) passes 32 (AES-256) and case 19 (FUN_140299A10) passes 24 (AES-192). | **Inferred**; constants **Observed** | T8 |
| 3.4 | Every transform works in place on one frame body of `n` bytes. It runs the `n div B` whole blocks through the family's block function with the key schedule at context+0x20, each block on its own (ECB), starting at the body's first byte. Then it XORs each of the last `n mod B` bytes, at index `i`, with `((n − i) mod 256) XOR C`, so the values count down from `(n mod B) XOR C` to `1 XOR C`. The XOR is its own inverse, so both directions apply the same tail step. For AES, `C` is 0xC6 in both transforms (0x140299C2C send, 0x140299B6C receive). | **Inferred**; constants **Observed** | T8, T9 |
| 3.5 | The key fold (FUN_14029BE20, `key, n, hello, 64`). For `n` below 64, which covers every AES case: `key[i mod n] ^= hello[i]` for `i` from 0 to 63, starting from zero; for AES-128, `key[j] = h[j] ^ h[j+16] ^ h[j+32] ^ h[j+48]`. For `n = 64` (case 17) the key is the hello. For `n` above 64 (cases 4, 9, 13) the hello fills the first 64 bytes and a generator the rest: `s` starts at `hello[0]`, each step sets `s = ((0x4473 − s × 0x3E39B193) mod 2^32 >> 16) AND 0x7FFF`, and the low byte of `s` is the next key byte. | **Inferred**; constants **Observed** | T7 |
| 3.6 | Both directions fold the same 64 bytes, so with case 3 both ways the two AES-128 keys are equal. For 2.6's hello the key is `03 00 00 00 33 7B 2F E6 00 00 00 00 00 00 00 00`. | **Inferred** (computed) | L2 |
| 3.7 | The server picks both variants through `w0` and `w1`, and Fenji picks case 3 both ways. The client's receive transform decrypts, so our server encrypts each frame body it sends (AES-128 encrypt on the whole blocks, then the tail XOR). The client's send transform encrypts, so our server decrypts each body it receives (AES-128 decrypt on the whole blocks, then the tail XOR). | **Inferred** | T8, T10, T11 |
| 3.8 | No variant reads `data/core/share/gate_dictionary.bin`: the inits take only the hello. That file feeds only the Oodle decoder (5.6) and the auth's 16 bytes (6.3). | **Inferred** | T6–T9, T14 |

## 4. Frames and keys

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.1 | After the hello, each direction is a sequence of frames. A frame starts with a `u16` LE length XORed with that direction's key `k`. A length of 0 marks a time-sync frame: 8 more bytes, four `u16` LE words each XORed with `k`, which together form one `u64` LE time in milliseconds; the variant doesn't touch it. Any other length is a data frame of that many body bytes, which the variant transforms (3.4). | **Inferred** | T10–T12 |
| 4.2 | Both keys start at 0. Each sits at +0x10C0 in its variant context (receive ctx+0x1480, send ctx+0x2548), which the variant selection clears after the hello. | **Inferred** | T6, T10, T11 |
| 4.3 | Server → client (FUN_14029B850): in each poll from state 1 on, the client handles every complete frame in its buffer and keeps an incomplete one for later. After each frame, data or time sync, `k = (k + 0xD736) mod 2^16` (0xD736 loaded at 0x14029B88F, added at 0x14029B904 and 0x14029B964). A data frame's body goes through the receive transform, then to the Oodle decoder (§5). From 0 the keys run 0x0000, 0xD736, 0xAE6C, 0x85A2, 0x5CD8 (L2). | **Inferred**; constant **Observed** | T10, L2 |
| 4.4 | A time sync from the server sets ctx+0x3B8 to the low 32 bits of the client's clock minus the frame's time. The NetClientLua method `fetch_ping_delay` returns that value (FUN_140221E20 → FUN_14029ABC0), which starts at 50; nothing else in the net client reads it. So a server that echoes the client's own time-sync value gives the client its round-trip time in milliseconds. | **Inferred** | T10, T16 |
| 4.5 | Client → server data frames (FUN_14029C600). The client cuts its outbound message stream into chunks of at most 0x800 bytes (0x14029C625). A chunk is smaller when the socket's send buffer is short: the client needs 0x80 bytes free to start a frame, and a chunk never exceeds the free space minus 2. Each chunk goes through the send transform, and the frame is `u16 (length XOR k)` followed by the transformed chunk. Then `k = ((k + 0x28CA) XOR k) mod 2^16` (0x28CA loaded at 0x14029C62A, used at 0x14029C6C0–0x14029C6CD). A data frame's length is never 0. From 0, with data frames only, the keys run 0x0000, 0x28CA, 0x795E, 0xDB76, 0xDF36 (L2). | **Inferred**; constants **Observed** | T11, L2 |
| 4.6 | Client time syncs (FUN_14029A5B0). At the start of each poll in a state above 0 with an open socket, the client sends one when more than 2,000 ms (0x7D0) have passed since the last, the first counting from `net_client_create`, and when its send buffer has 10 bytes free. The frame is `u16 k` (length 0 XOR `k`), then the four 16-bit words of its clock, lowest first, each XORed with `k`; the variant doesn't touch it. Then `k = ((k − 0x4B05) XOR k) mod 2^16` (0x4B05 at 0x14029A626), which equals `((k + 0xB4FB) XOR k)` for every `k` (L2). The clock (FUN_1400B5670) is Unix time in milliseconds, from `GetSystemTimePreciseAsFileTime` or `GetSystemTimeAsFileTime`. | **Inferred**; constants **Observed** | T12, L2 |
| 4.7 | Each direction has one key, stepped once per frame in sending order by that frame's kind. On the client's side, data frames and time syncs interleave on one key, so our server must step its copy by the data step or the time-sync step according to each frame's decoded length. | **Inferred** | T11, T12 |
| 4.8 | The client's auth goes out in its first data frames after the hello (§6), the first of them with `k = 0`. Its first time sync can come only in a later poll. | **Inferred** | T5, T12 |
| 4.9 | Limits for our server: a frame body of at most 0x4080 bytes (5.4), and never a data frame of length 0, which the client would read as a time sync. | **Inferred** | T10, T13 |

## 5. The Oodle wrapper

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 5.1 | After its receive transform, every server data frame goes to FUN_14029B0A0, and nothing lets a frame skip the decoder. Client frames never pass through Oodle: FUN_14029C600 and FUN_14029A5B0 call no Oodle function. | **Inferred** | T10, T11, T12, T13 |
| 5.2 | The body starts with a raw-length prefix (table below; compares at 0x14029B0B8–0x14029B0EA). | **Inferred**; constants **Observed** | T13 |
| 5.3 | The decode: `OodleNetwork1TCP_Decode(state, shared, body + prefix, body length − prefix length, output, raw length)` (the import called at 0x14029B133). On success the client takes the raw length as the decoded size and appends that many bytes to its inbound message stream (ctx+0x390, FUN_14029A7D0). A failed decode sets error 0x5F5E101 and closes the socket, and a failed append sets 0x5F5E102. Nothing checks that the body is longer than its prefix, or that the raw length fits the output buffer. | **Inferred**; constants **Observed** | T10, T13 |
| 5.4 | Buffer sizes. `net_client_create` passes no buffers of its own, so FUN_14029A900 uses two globals 0x4080 bytes apart: the body is copied into DAT_143200990 and decoded into DAT_1431FC910. The next global after them is at 0x143204A24, so each holds 0x4080 bytes. The client checks neither size, so our server must keep each body, and each raw length, at or below 0x4080 (16,512) bytes. | **Inferred** | T13 |
| 5.5 | A raw length of 0 (prefix `00`, or `FF`): if the decoder succeeds, nothing is appended and nothing fails; if it fails, error 0x5F5E101. | **Inferred** | T13 |
| 5.6 | One decoder state per connection (FUN_14029B020): a copy (`OodleNetwork1TCP_State_InitAsCopy`) of the state the client trained at start-up from `gate_dictionary.bin`, used with the shared dictionary built from the same file (FUN_14029B150). The state is passed to every decode, so our encoder would have to start from the same dictionary and trained state and encode every frame in order. | **Inferred** | T14 |
| 5.7 | The dictionary image. The net module's set-up (FUN_140222740) loads `data/core/share/gate_dictionary.bin` whole (FUN_140222360). Its first 16 bytes must equal the MD5 of the file with those 16 bytes zeroed (FUN_1400B5B30: the MD5 start values at 0x1400B5B59–0x1400B5B6E, and its first round constant 0xD76AA478 at 0x1400B5C5E); otherwise the file is rejected. FUN_14029B150 then reads the window size (`u32` at +0x14), a byte at +0x12 that it passes to `OodleNetwork1_Shared_Size` and `OodleNetwork1_Shared_SetWindow`, the window from +0x1C, and the trained state after the window (rounded up to 32 bytes). Without a valid file, FUN_14029A6A0 builds a stand-in dictionary from one built-in 1-byte packet (0x1411ADEE8). | **Inferred**; constants **Observed** | T14 |
| 5.8 | A stored packet passes the decoder: a body whose raw-length prefix equals the length of the bytes after it, those bytes unencoded. In runs m2-r3 and m2-r3b the client decoded one such packet, the first on its connection with a one-byte prefix (7 and 39 bytes of body), and parsed the message it held. A stored `ok` kept the client connected for the whole run. A stored rejection closed it at once, logged as `NET_ST_CLOSED_AUTH(7)` with our reason, and showed our text (§9). In m2-r4 and m2-r4b the client also decoded a second stored packet, with 17 and 87 bytes of body. That the decoder accepts every stored packet, longer prefixes included, so that the server needs no Oodle Network encoder, is our inference. What the decoder does with a raw length of 0 is still **Unknown**. | Runs **Observed**; the generalisation **Inferred** | M2R3, M2R3B, M2R4, M2R4B |
| 5.9 | Not the network: the client's one `OodleLZ_Decompress` call site (FUN_14024E060, one of spike-0 4.6's wrappers) passes 14 arguments: the block plus 4, the block's length minus 4, the output, the `u32` read from the block's first 4 bytes as the raw length, then 1, eight zeros and 3 (call at 0x14024E0AB). So that wrapper's blocks start with a `u32` raw length. Whether the archives' codec-4 blocks use it is Task 11's question. | **Inferred**; constants **Observed** | T17 |

| First byte `b` | Prefix bytes | Raw length |
|---|---|---|
| 0x00–0xFB | 1 | `b` |
| 0xFC | 3 | `u16` LE after it |
| 0xFD | 4 | 24-bit LE after it |
| 0xFE | 5 | `u32` LE after it |
| 0xFF | 0 | 0, and the `FF` byte stays in the data the decoder gets |

## 6. Auth

FUN_14029C330 builds the client's first message as soon as the hello passes, and sends it at once:

| Offset | Bytes | Content |
|---|---|---|
| 0 | 4 | Inner header (§7): type 0, length `22 + payload length` |
| 4 | 16 | The first 16 bytes of the dictionary image that DAT_1431FC8E0 points to (6.3) |
| 20 | 6 | The machine bytes (6.2) |
| 26 | `n` | The payload given to `net_client_create`, at most 0x4000 bytes |

| Case | The 6 bytes |
|---|---|
| `getsockname` and `SendARP` succeed | The MAC address `SendARP` returns for the socket's own local IPv4 address |
| `SendARP` fails | `FF 00`, then `SendARP`'s error as `u32` LE (0x14029C506) |
| `getsockname` fails | `FE 00 00 00 00 00` (0x14029C526) |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 6.1 | The layout above: the 26-byte head (0x1A at 0x14029C547) goes into the outbound stream, then the payload. Both tables are code readings. | **Inferred**; constants **Observed** | T5 |
| 6.2 | The 6 bytes come from `getsockname` (FUN_1400C41C0) on the client's socket, then `SendARP` (called at 0x14029C4D0) with that local address as the destination. | **Inferred** | T5 |
| 6.3 | DAT_1431FC8E0 points to the dictionary image (5.7). FUN_14029AB30 sets it, and the net module's set-up is its only caller. With a valid `gate_dictionary.bin`, the 16 bytes are the file's stored MD5 digest; with the stand-in dictionary they are 16 zero bytes. Either way they tell the server which dictionary the client's decoder uses. In every run so far the 16 bytes were zero (§9). Whether that means the stand-in dictionary, or a valid file whose digest bytes stay zeroed because nothing writes the saved digest back, is **Unknown**. | **Inferred**; zeros at run time **Observed** | T5, T14, M2R1, M2R3, M2R3B |
| 6.4 | The payload is the login data the scripts packed with `ks.pickle_pack` ([`login.md`](login.md) 2.6). `ks.pickle_pack` is FUN_1400D5AC0: FUN_1400D7660 pushes it at 0x1400D777E and names it `pickle_pack` at 0x1400D77AB. The C-level export `kslua_pickle_pack` is FUN_1400D54F0. The format is Task 9's. | **Inferred**; names **Observed** | T18 |
| 6.5 | If either append fails (out of memory), the client closes the socket and sets state 4, with no error code of its own. | **Inferred** | T5 |
| 6.6 | The reply (FUN_14029ADD0, state 1). After reading frames, each poll looks at the first inbound message once at least 5 bytes are buffered and the whole message has arrived; it doesn't read the type byte. A 2-byte body equal to `ok` (`6F 6B`, the constant at 0x1411ADEEC) sets state 2 `QUEUE` (0x14029AF11), clears the close reason and frames any queued data. Anything else ends the connection: the body, cut to 255 bytes and NUL-terminated, becomes the close reason (ctx+0x270), the error is 0x5F5E106, the socket closes, and the state is 7 `CLOSED_AUTH` (0x14029AFD5). Messages after `ok` stay in the inbound stream for state 2 (7.4). | **Inferred**; constants **Observed** | T3, T19 |
| 6.7 | So the server's `ok` message, before the Oodle encoding and the transform, is `00 01 02 00 6F 6B` with type 0 (header byte 1 is `0 XOR (2 >> 1)`). The client accepts any type there, since it doesn't read it. | **Inferred** | T3, T15 |

## 7. Inner messages

| Byte | Content |
|---|---|
| 0 | Length, bits 16–23 |
| 1 | Type XOR (length >> 1), low 8 bits |
| 2 | Length, bits 0–7 |
| 3 | Length, bits 8–15 |

The length counts the body only, up to 0xFFFFFF.

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 7.1 | The header above, as written by FUN_14029AC60 (outbound messages) and FUN_14029C330 (the auth, type 0), and as read by FUN_14029ABE0, which recovers the type as byte 1 XOR `(length >> 1)`. Matches spike-0 4.29. | **Inferred** | T5, T15 |
| 7.2 | Messages and frames are independent. The client's outbound messages form one stream, which it cuts into frames (4.5); the server's decoded frames append to one inbound stream (5.3), which the client splits into messages. A message can span frames, and a frame can hold several messages. | **Inferred** | T11, T13, T15 |
| 7.3 | The client queues an outbound message only in state 3 `WORK`: FUN_14029AC60 returns without sending in any other state. The NetClientLua method table (0x143114860) names `__gc`, `destroy`, `set_working`, `send_gateway`, `send`, `send_time_sync`, `send_enter`, `send_leave`, `send_sync_filter` and `fetch_ping_delay`. `send_time_sync` (FUN_140221AC0) posts a 13-byte message of type 2 through the network thread. | Names **Observed**; the rest **Inferred** | T15, T16 |
| 7.4 | Inbound, in states 2 and 3, the network thread reads each complete message. Type 1 closes the connection (FUN_14029A870): the body becomes the close reason (255 bytes kept), the socket closes and the state is 5. Every other type goes to the scripts as type and body, through `net_client_dispatch`. | **Inferred** | T4, T15 |

## 8. Open questions

- **`QUEUE` → `WORK`.** Answered: the script message `PROTOG2C_SET_NET_CONNECTED` (§10), seen to work in runs m2-r4 to m2-r5 (§9).
- **Payload encoding.** Answered for the script opcodes and the auth payload: the client's pickle, LZ4-compressed from 128 bytes up ([`payloads.md`](payloads.md)). The other opcodes' bodies are per-type native formats ([`opcodes.md`](opcodes.md)).
- **Server time syncs.** Nothing found waits for one. The client's scripts start sending type-2 time-sync messages once a character is in the world, and the reply's format is read (10.6).
- **Script message 402,** sent after `WORK`: its name and what the gate answers.
- **R3.** Answered as far as tested: stored packets decode, a later and longer one under the stepped key included (5.8, §9).
- **The 16 auth bytes.** Zero in every run (§9); whether that means the stand-in dictionary is open (6.3).
- **The real server's choices.** The `ok` message's type (6.7), and what the server did with the 16 bytes and the MAC address.

## 9. Runs

M2 plan Tasks 8 and 10 ran the pinned client against `fenji-stub --speak` inside `scripts/spike/run-client.sh`. Every run used:
- the sandbox options `--no-root --x11 99 --cpu-topology`, with the AGT driver watchdog armed;
- the stub on 127.0.0.1:7800 with variants 3 and 3;
- the client arguments `--gf-sdk=mocked --fs=data:_index:_index`, and one of two login paths:
  - m2-r1 to m2-r4b, the launcher path ([`login.md`](login.md) 2.8–2.10): the environment variable `gj_uinfo=1:90:Fenji:0:tester:1`, and the client arguments `gj_gates=127.0.0.1:7800 gj_ticket=fenji-dev`;
  - m2-r5, the server-list path ([`login.md`](login.md) §3), as run m1-c3: a local server list (`serverlist_url=file:///…/serverlist.json`) whose one entry points at the stub, then two clicks, on the entry and on the confirm button. The mocked SDK supplies the ticket and the language (German).

| Run | The stub's reply to the auth | What the stub saw | Verdict | Watchdog | Exit |
|---|---|---|---|---|---|
| m2-r1 | none (`--auth-reply=none`) | hello sent; one 101-byte data frame (length 99 under key 0) holding the auth (type 0, length 95: 16 zero bytes, `FF 00 32 00 00 00`, a 73-byte payload); then 85 time syncs, one every 2,000 ms; closed by the run's timeout | R1 and R2 pass | no sign | 0 |
| m2-r3 | `ok` in a stored frame | as m2-r1, then the client stayed connected for the rest of the run (85 time syncs, 170 s) with no close | no decode failure | no sign | 0 |
| m2-r3b | a rejection reason in a stored frame | the hello and the auth as in m2-r1; then the client closed 5 ms after the reply, logged `net error during waiting: 7 fenji-stub: rejected (control run)` and `Net State: NET_ST_CLOSED_AUTH(7), Close Reason: fenji-stub: rejected (control run)`, and showed an "Error" dialog, "Internal server error: fenji-stub: rejected (control run)" | stored frames are decoded and parsed: R3 passes | no sign | 0 |
| m2-r4 | `ok`, then `PROTOG2C_SET_NET_CONNECTED` with an empty billing table (10.4), each in a stored frame | the hello and the auth as in m2-r1; 2.3 s after our two frames, one message of type 0xFF whose body is the pickle of (402), `01 04 92 01`; then time syncs until the timeout. `gujianol.log`: `[net]net connected...`, then a script error from a listener of `EVENT_NET_CONNECTED` that formats a billing number (`bad argument #1 to '?' (number expected, got nil)`) | the client reached `WORK` and sent its first script message: R4 passes | no sign | 0 |
| m2-r4b | as m2-r4, with the four billing fields set to 0 | as m2-r4: the message with 402 at 2.3 s; `[net]net connected...` and no script error | R4 passes, without the error | no sign | 0 |
| m2-r5 | as m2-r4b | the hello; an auth whose payload is 285 bytes, LZ4-compressed ([`payloads.md`](payloads.md) P.6); 2.3 s after our two frames, the message with 402; `[net]net connected...` and no script error | the server-list path reaches `WORK` too: R5 passes | no sign | 0 |

**Observed:**
- **The hello passes.** The client accepted a hello built by `fenji_proto::hello::build` with w0 % 27 = w1 % 27 = 3 (§2), and answered with its auth message.
- **The variant choice is unsigned.** m2-r3's w0 (0xFF05AA95) and m2-r3b's w1 (0x998825C7) had the top bit set, and both directions still worked: the client decoded our stored replies, and its auth decoded with variant 3. So `% 27` is taken on the unsigned word (§2, §3).
- **The client's frames decode with our reading.** The fixture `crates/fenji-proto/tests/fixtures/r2-*.hex` holds m2-r1's real hello and auth frame, scrubbed (below). `fenji-proto` decodes it with the AES-128 variant, the key fold, the up key and the inner header (§3, §4, §7). An independent decryption with `openssl` matched before the fixture was made (L4).
- **The auth's fixed part.** The 16 dictionary bytes were zero in every run (6.3). The 6 machine bytes were `FF 00 32 00 00 00`: `SendARP` failed with error 50 (`ERROR_NOT_SUPPORTED`) inside the sandbox, so no MAC address was sent (6.2). The payload was 73 bytes on the launcher path and 285 bytes, compressed, on the server-list path (6.4).
- **Time syncs.** The client sent one every 2,000 ms, before `ok`, after it and in `WORK` (4.6). It sent no type-2 time-sync message, as 10.6 expects before a character is in the world.
- **Stored packets.** A stored `ok` kept the client connected, and a stored rejection reason closed it at once, was logged with state 7 `CLOSED_AUTH`, and was shown to the player (5.8, 6.6). By 5.3's reading, a decode failure would have closed the connection with 0x5F5E101 at once.
- **Later and longer stored packets, and the server's key step.** In m2-r4 and m2-r4b the second stored packet on the connection, 17 and 87 bytes of body, went out under the stepped key 0xD736 (4.3), and the client acted on it.
- **`WORK`.** After `PROTOG2C_SET_NET_CONNECTED` the client logged `[net]net connected...` and sent the script message 402 to the gate (`CTS_PROTOCOL_GATE_SCRIPT`) with no arguments, on both login paths: 10.3–10.4 hold. The fixture `crates/fenji-proto/tests/fixtures/r4b-*.hex` holds m2-r4b's hello, our two frames and the client's first three frames, scrubbed. A test checks that `fenji-proto`'s server sends exactly our two frames for that hello, and that it decodes the client's time sync and its message with 402.

**Inferred:**
- **`QUEUE`.** After the stored `ok`, the client is in state 2, `QUEUE` (6.6). It logs its network state only when the login fails (m2-r3b named state 7, `CLOSED_AUTH`). After `ok` it logged nothing, as with no reply in m2-r1, so state 2 itself wasn't seen.

**The fixtures are scrubbed.** They keep the real hellos, frame lengths, inner headers, 16 dictionary bytes and login values: the run's own `gj_uinfo` and `gj_ticket` values and facts about the client (its version and language). The 6 machine bytes are replaced with `02 00 00 00 00 01`, and the payload's 20-byte machine fingerprint ([`payloads.md`](payloads.md), the auth payload, value 9) with zeros. Then each auth frame is re-encrypted with its hello (L4). `tests/fixtures.rs` checks the decoding and the scrubbing. m2-r5's compressed payload isn't a fixture: an ignored test decodes it from the local run log.

## 10. After auth

From `QUEUE` on, the client's inbound messages go to its scripts, and one script message moves it to `WORK`. Opcode values are in [`opcodes.md`](opcodes.md), and the pickle format is in [`payloads.md`](payloads.md).

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 10.1 | **Dispatch.** In states 2 and 3 the network thread (FUN_1402234D0) splits each inbound message off the stream (FUN_14029ABE0). Type 1 closes the connection (7.4). Every other message is posted to the scripts' queue as an event. `net_client_dispatch` (FUN_140222AD0) turns each event into a call `handler(client, type, values…)`: FUN_140222E00 pushes the type and decodes the body for it (a type-0 body is unpickled). The handler is the function `net_manager_update` passes in (`data/core/client/net_manager.luac`). | **Inferred** | T1, T4, T15; FUN_1402234D0, FUN_140222E00; net_manager.luac |
| 10.2 | **Script messages.** For type 0, `STC_PROTOCOL_SCRIPT`, the handler takes the first unpickled value as a message id and calls the script handler registered for it, with the remaining values as its arguments. An id with no handler is logged ("[net]not find script msg handle:"). Other types go to a second table of handlers by type. | **Inferred** | net_manager.luac |
| 10.3 | **`QUEUE` → `WORK`.** The script handler for id 414 logs `----PROTOG2C_SET_NET_CONNECTED--------`. It then stores the billing fields from its second argument and the user id from its third, and passes its first (a time-zone offset) to `ks.time_zone_offset_setup` when it isn't `nil`. Next it calls `net_manager_set_working`, the NetClientLua method `set_working` (FUN_1402218C0): the network thread then sets state 3 (FUN_14029AD60). Finally it logs "[net]net connected...", fires `EVENT_NET_CONNECTED` and sends script message 402 to the gate (`CTS_PROTOCOL_GATE_SCRIPT`). | **Inferred**; `[net]net connected...` and the message with 402 **Observed** (§9). The handler's own `----PROTOG2C_SET_NET_CONNECTED--------` line appeared in no run's log | `data/core/modules/net/net.luac`; T16; FUN_1402234D0; M2R4 |
| 10.4 | **So the server moves the client to `WORK`** by sending, after `ok`, one message of type 0 whose body is the pickle of (414, time-zone offset or `nil`, a table, user id or `nil`). The client answers with a type-0xFF message whose body is the pickle of (402), `01 04 92 01`. In run m2-r4 an empty table, the body `04 04 9E 01 00 0B 00 00 00 00 00 00`, moved the client to `WORK`, but a listener of `EVENT_NET_CONNECTED` failed on a missing billing number. So Fenji sends the four billing fields as 0 (`fenji_proto::packet::set_net_connected`, [`payloads.md`](payloads.md), Script messages), which ran without the error in m2-r4b and m2-r5. | **Observed** for (414, `nil`, the billing table, `nil`); the other argument forms **Inferred** (10.3) | 10.2, 10.3, M2R4, M2R4B, M2R5 |
| 10.5 | **Client sends.** `send_gateway` and `send` pickle the message id and arguments (FUN_1400D54F0), refusing an id that isn't a number or a string ("script PT type error: %d"), and post them with type 0xFF or 0. `send_time_sync`, `send_enter`, `send_leave` and `send_sync_filter` post fixed-size bodies of types 2, 3, 4 and 5 ([`opcodes.md`](opcodes.md)). The network thread sends them only in `WORK` (7.3). | **Inferred**; type constants **Observed** | FUN_140221940, FUN_140221A00, FUN_1402226B0, T16 |
| 10.6 | **Time sync messages.** Nothing in the transport or in `net_manager_update` waits for one. The client's scripts send `CTS_PROTOCOL_TIME_SYNC` (type 2) through `net_manager_send_time_sync` once a second (`SERVER_TIME_SYNC_INTERVAL`, 1000 ms), but only while a local actor exists (`time_manager_update`, `data/core/client/time_manager.luac`), so not before a character is in the world. Its 13-byte body (`send_time_sync`, FUN_140221AC0) is the client's millisecond clock (`u32`, FUN_1400B5670), a byte 1, and two `u32` values from FUN_140246E40 and FUN_140246DF0 (meaning **Unknown**). An inbound `STC_PROTOCOL_TIME_SYNC` body is read as a `u32` and a `u64` (FUN_140222E00, case 2), with two effects. Natively, when the `u32` is below the client's clock, the client updates a smoothed round trip (7/8 old, 1/8 new) and the server-time estimate that `net_client_get_server_time` returns. And the client pushes (its clock now, the `u32`, the `u64`) to the scripts' handler for type 2, `time_manager_time_sync_resp`, which sets `net_delay` to now − `u32` and `server_time_now` to `u64` + `net_delay` / 2. So the reply echoes the client's `u32` and adds the server's time in milliseconds. The 2-second transport-level time-sync frames (4.6) are separate from these messages. `fenji-stub --speak` doesn't answer type-2 messages, since none arrive before a character is in the world. | **Inferred** | FUN_140222E00, T12, T16; `time_manager.luac`, `net_manager.luac` |
| 10.7 | **Script message 556** carries a chunk of Lua bytecode from the server, which the client loads and runs, answering with message 278 and the chunk's MD5. Fenji has no use for it. | **Inferred** | net_manager.luac |

## Evidence index

`T` keys are functions and data in the pinned exe; `re/` is git-ignored, so the `L` files exist only on the owner's machines.

| Key | Address or path | What it holds |
|---|---|---|
| T1 | FUN_140222740 | The net module's set-up: loads `gate_dictionary.bin` (FUN_140222360) and calls FUN_14029AB30; registers `KS_NET_CLIENT_ST_*` with 0–7 and the bindings `net_client_create` (FUN_140222970), `net_client_dispatch` (FUN_140222AD0), `net_client_debug` (0x140222AC0) and `net_client_get_server_time` (FUN_140222DE0) |
| T2 | FUN_140222970, FUN_14029A900, FUN_140299940, FUN_1400C4200 | `net_client_create`; client creation (`K` at ctx+0x50, payload, buffers); gate list and address parsing |
| T3 | FUN_14029ADD0, FUN_14029C030, FUN_1400C4370, FUN_1400C44D0, FUN_1400C44A0 | The poll (`IN_AUTH` reply at 0x14029AE6E–0x14029AFD5); connect with its 20-second timeout; the socket pump; close |
| T4 | FUN_1402234D0, FUN_140222AD0, FUN_14029A8D0, FUN_14029A8E0, FUN_14029AAB0 | The network thread; `net_client_dispatch`; the error-code and close-reason getters; client teardown |
| T5 | FUN_14029C330 (0x14029C38F–0x14029C5C2) | The hello check, the variant set-up, the auth message and the 6 bytes |
| T6 | FUN_14029BA10, FUN_14008E470 | Variant selection; the empty slot-0 function |
| T7 | FUN_14029BE20 | The key fold |
| T8 | Inits FUN_140299C50, FUN_140299A70, FUN_140299A10; transforms FUN_140299B90, FUN_140299AD0; AES FUN_14008EBC0, FUN_14008F3B0, FUN_14008FA10; tables 0x142EF0060, 0x142EEF860, 0x142EEEC34; string 0x142EFC398 | The AES family |
| T9 | Inits: 0 FUN_14029C090, 1 FUN_14029B4E0, 2 FUN_14029A0D0, 4 FUN_14029B540, 5 FUN_14029B290, 6 FUN_14029A310, 7 FUN_14029A370, 8 FUN_140299E30, 9 FUN_14029B220, 11 FUN_14029B7F0, 12 FUN_140299E90, 13 FUN_14029B2F0, 14 FUN_140299D70, 15 FUN_14029A550, 16 FUN_140299DD0, 17 FUN_14029B610, 18 FUN_14029A070, 20 FUN_14029C2D0, 21 FUN_14029B5B0, 22 FUN_14029A130, 23 FUN_14029C0F0, 24 FUN_140299CB0, 25 FUN_14029C8E0, 26 FUN_140299D10; transforms and cipher functions as in §3's table; strings 0x142EEC2F0, 0x142EFC368, 0x142EFC380, 0x142EFC3B0, 0x142EFC3C8, 0x142EFC3E8, 0x142EFC400 | The other seven families |
| T10 | FUN_14029B850 | Server → client frames, the key step, server time syncs |
| T11 | FUN_14029C600 | Client → server data frames and the key step |
| T12 | FUN_14029A5B0, FUN_1400B5670, FUN_1400B55B0 | Client time syncs; the millisecond clock |
| T13 | FUN_14029B0A0, FUN_14029A7D0; DAT_1431FC910, DAT_143200990 | The raw-length prefix and `OodleNetwork1TCP_Decode`; the inbound-stream append; the two 0x4080-byte buffers |
| T14 | FUN_14029AB30, FUN_14029AD80, FUN_140222360, FUN_1400B5B30 (with FUN_1400B5A40, FUN_1400B5C20), FUN_14029A6A0, FUN_14029B150, FUN_14029B020; DAT_1431FC8E0 | The dictionary image, its MD5 check, the stand-in dictionary, the shared dictionary and the per-connection decoder state |
| T15 | FUN_14029AC60, FUN_14029ABE0, FUN_14029A870 | Inner messages out (state 3 only) and in; the type-1 close |
| T16 | Method table 0x143114860 (names at 0x142EFC808, 0x142EFEEF0, 0x14118C970–0x14118C9D8, 0x142EFFCA8); FUN_1402218C0 (`set_working`), FUN_14029AD60, FUN_140221E20 with FUN_14029ABC0 (`fetch_ping_delay`), FUN_140221AC0 (`send_time_sync`) | The NetClientLua methods |
| T17 | FUN_14024E060 | The `OodleLZ_Decompress` wrapper |
| T18 | FUN_1400D7660, FUN_1400D5AC0, FUN_1400D54F0 | The `pickle_*` registration; `ks.pickle_pack`; `kslua_pickle_pack` |
| T19 | 0x1411ADEEC | The `ok` constant (`6F 6B`) |
| L1 | `re/m2/transport/checks.md` | The re-verification of F1–F8, finding by finding, with the addresses read, and the searches behind the error-code table |
| L2 | `re/m2/transport/div14.c` with `div14.out`; `re/m2/transport/example.py` with `example.out` | Our own arithmetic checks: the index over all 2^32 values of `w1`, the key-step identities over all 2^16 keys, the key sequences, and the worked hello and key of 2.6 and 3.6 |
| M2R1 | `re/stub-logs/m2-r1/` (`.log`, `.events`, `.auth.bin`), `re/runs/out/m2-r1/`, `re/m2/m2-r1.console` | Run m2-r1: the hello accepted, the auth captured |
| M2R3 | `re/stub-logs/m2-r3/`, `re/runs/out/m2-r3/`, `re/m2/m2-r3.console` | Run m2-r3: the stored `ok` |
| M2R3B | `re/stub-logs/m2-r3b/`, `re/runs/out/m2-r3b/` (`gujianol.log` lines 59–60, the dialog in `screen-120s.png`), `re/m2/m2-r3b.console` | Run m2-r3b: the stored rejection, closed, logged with state 7 and shown |
| M2R4 | `re/stub-logs/m2-r4/`, `re/runs/out/m2-r4/` (`gujianol.log` line 59, `[net]net connected...`; lines 60 and 75, the listener's error), `re/m2/m2-r4.console` | Run m2-r4: `WORK` and the message with 402, with an empty billing table |
| M2R4B | `re/stub-logs/m2-r4b/`, `re/runs/out/m2-r4b/` (`gujianol.log` line 59), `re/m2/m2-r4b.console` | Run m2-r4b: as m2-r4, the billing fields 0, no error; the source of the `r4b` fixtures |
| M2R5 | `re/stub-logs/m2-r5/` (`.auth.bin`: the compressed payload), `re/runs/out/m2-r5/` (`gujianol.log` line 57), `re/m2/m2-r5.console` | Run m2-r5: the server-list path to `WORK` |
| L3 | `re/m2/runs.md` | Each run's prediction, written before it, and its outcome |
| L4 | `re/m2/scrub_r2.py`, `re/m2/scrub_r2_fields.py`, `re/m2/fixtures_r4b.py` | The independent decryption (Python and `openssl`) and the scrubbing that made the fixtures |
