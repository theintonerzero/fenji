# Chat: saying, hearing, refusals and the admins' lines

In the world, a player's line goes to the server as the script message 115, and every line a client shows comes from the server as 378, its own included: the client never shows a line it sends. A refusal is the 270 every error uses, with a code from the client's `errors` sheet. The admins' commands (`!tp`, `!kick`, `!announce`, `!invite`) ride on ordinary lines, since lines that start with `/` never leave the client. This document gives the evidence behind `fenji_proto::chat` and the link's mapping (§6), and records the runs with two real clients (§7).

**Method.** This reading comes from the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`). It used three sources, all read locally and read-only:
- the client's own scripts, disassembled and decompiled by `luatool` ([`../../re/m1-script-format.md`](../../re/m1-script-format.md));
- the client's own caches: the protocol table, `prop_tables`, the `errors` sheet of `all_tables` and `locale_en`, read with standard-library probes of [`../payloads.md`](../payloads.md)'s format;
- the exe, through REA in one session (2026-10-15), after the evidence norm ([`../../re/README.md`](../../re/README.md#the-norm)), for how the client packs a script message's values. Its IDs are in [REA Evidence IDs](#rea-evidence-ids).

A design-time survey (L4, its chat part) gave the leads, and each finding here was checked against the bytecode, a cache or the exe. Task 16's runs (§7) then checked them against the real client. Nothing in the shared Ghidra project was renamed or changed, and no script ran in Ghidra. Written by a session that has not read any source rejected in [`../../re/sources.md`](../../re/sources.md). Tags follow [`../../re/spike-0.md`](../../re/spike-0.md): **Observed** (literally in a file: a constant, a bytecode operand, a table value, bytes of the exe), **Inferred** (our reading of the code), **Unknown**. Script evidence cites the engine path and the function path in its disassembly (such as `0.8`), with bytecode positions in brackets; where the decompiled text and the bytecode differ, the bytecode wins. Evidence keys resolve in the [Evidence index](#evidence-index).

## 1. Saying something: 115 `PROTOC2S_CHAT_TRANSMIT`

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 1.1 | **The path.** The chat input's Enter calls `SendChatMessage` (S3 `0.19`), which sends only a line that isn't blank once spaces are removed, and calls `CHAT.SendMessageToServer(channel, inputLine:GetHtmlText(), extra)`. That function (S1 `0.8`) takes the channel through `math.tointeger` and returns unless it is 0 to 17 [1]–[13], returns unless the text is a string [14]–[19], hands a line that starts with `/` to the local commands [20]–[30] (so it never leaves the client), runs `CheckTransformChatMsg` (1.5) [31]–[36] and the quest special case [37]–[44], then calls `net_manager_send(115, channel, text, extra)` [45]–[50]. `net_manager_send` (S2 `0.25`) sends through the network client's `send` method, a type-0 script message. Nothing shows the player's own line: the server echoes it. | **Observed** (bytecode) | S3 `0.19`; S1 `0.8`; S2 `0.25`; X1 |
| 1.2 | **Four values: the `extra` is always there, nil or not.** The call at S1 `0.8` [50] is `CALL 6 5 1`: four arguments, the last `extra`, which is nil for every channel but a whisper. `net_manager_send` passes them on with `VARARG` and `B = 0` [27]–[28], which keeps a trailing nil. The native `send` (FUN_140221A00) packs from the stack's index 2 to its top: its call to the packer, FUN_1400D54F0, sets `R8` to 2 and `R9` to −1 (0x140221A4D, 0x140221A56), and the packer makes a negative index absolute against the top and packs every value in between (FUN_1400D5280), a nil as `00` ([`../payloads.md`](../payloads.md) P.3). So a nearby line is `(115, 0, text, nil)`. Only a line sent while sending is off goes through the cache (`{...}`, then `table.unpack`), which drops a trailing nil; chat is sent after type 8, when sending is on. Every 115 of Task 16's runs had the four values (§7). | **Observed** (bytecode, instructions; runs), **Inferred** (the cache) | S1 `0.8`; S2 `0.25`; X1; §7 (T16C1) |
| 1.3 | **The channels the input sends.** The input starts on 0 (Normal). A `/` word and a space switch it (S3 `0.7`): `/s` Normal (0), `/w` Whisper (1), `/p` Team (2), `/c` Area (4), `/y` World (5), `/g` Alliance (6) and the rest of the client's channel table (S10 `0`). M4 carries 0, 1 and 5. | **Observed** (bytecode, the table) | S3 `0.7`; S10 `0`; S16 |
| 1.4 | **A whisper's target, `extra`.** For channel 1, `extra` is the input's whisper target, a table with `pid` and `name` (S3 `0.18`); for any other channel, nil. The target's setter (S3 `0.4`) appends `@` and a server name to a name without one, `NET.GetServerNameByID` of the player's own group, and stores both fields. Typed with `/w Name ` (S3 `0.7` [48]–[56]), the pid is nil, so the table holds `name` alone. Set by clicking another player's name in a line (a left click, S6 `0.14` [29]–[33]; a right click opens the player menu instead [38]–[41], whose Whisper does the same, `0.9`, `0.9.1`) or a whisper line's label (S6 `0.7`), `SetPersonalChannel` (S3 `0.10`) takes the pid and the group from the link through `tonumber`, so the table holds the pid as an integer and the name with the group's server name (`"{1}@{2}"` [23]). The server-name map comes from 495, which Fenji doesn't send, so the name of the server is `Unknown` (`_L("未知")`, S9 `0.17`): `Mai@Unknown`. A table makes the packet's header `0x80` OR the count, then a three-byte table count ([`../payloads.md`](../payloads.md) P.7). t16-c1's two whispers carried exactly these (§7). | **Observed** (bytecode; runs) | S3; S6; S9 `0.17`; D4; §7 (T16C1) |
| 1.5 | **The text's form.** `CheckTransformChatMsg` (S1 `0.7`) removes control characters, refuses over 140 characters as the client counts them (`UI_PARSE.GetTextActualLength`: characters, and 10 for each link), more than 10 links and an emoji the player hasn't unlocked (S1 `0.6`, §4), turns the input's links into `[link …]` markup, writes `&`, `<` and `>` as `&amp;`, `&lt;` and `&gt;` (`EscapeHtmlChar`, S15 `0` [55]–[59]), masks words from its lists, and refuses over 2,048 bytes. A `[` the player types stays as it is. The server takes none of this on trust: the world checks every line again (the world spec §3.5; the Global Constraints' chat rule). | **Observed** (bytecode) | S1 `0.7`, `0.6`; S15 |
| 1.6 | **What Fenji takes from a 115** (`chat::parse_transmit`): the channel, which must be the integer 0, 1 or 5, else `ERR_CHAT_INVALID_CHANNEL`; the text, which must be a string of at most the client's own 2,048 bytes (1.5), else `ERR_CHAT_MAX_CHAR`, and of UTF-8, else `ERR_CHAT_INVALID_CONTENT`; and for a whisper, `extra`'s pid when it is a whole number, else its name, of at most 64 bytes, else `ERR_CHAT_MAX_CHAR`. Each size is checked before the bytes are read or copied (#280). A nil after the text and no value at all are the same, and values after `extra` are ignored. The link answers those refusals at once, as it answers a time sync, so none of those lines reaches the world. The rest goes to the world, which applies the rate, the length and the text rule. | the choices are Fenji's | 1.2–1.5 |

## 2. Hearing it: 378 `PROTOS2C_SYS_CHAT_MESSAGE`

| Argument | Type | Use | Fenji sends | Evidence |
|---|---|---|---|---|
| channel | integer | the line's routing, label, colour and tab (§3) | 0, 1 or 5 for a player's line; 64 or 66 for the server's (§3) | S4 `0.10`; S5 `0.27`; S6 `0.4`, `0.5` |
| sender | string, nil read as `""` | the player link (2.2): its pid for the blacklist, the bubble and "is it me"; the name shown | the speaker's link; `""` for the server's lines | S4 `0.10`; S5 `0.29`; S6 `0.5` |
| content | string; nil or `""` drops the line | the text, through the label's markup (2.5) | the line as the client wrote it; the server's own text made to show as written | S4 `0.16` [1]–[10]; S14 `0.0` |
| receiver | string or nil | shown only on channel 1, in "You tell {1}" | the target's link for a whisper, `""` otherwise | S6 `0.5` [29] |
| options | table or nil | `mask`, `out_chatbox`, `ui_show_range`, `chatbox_show_range` | nil | S4 `0.10`, `0.5` |
| template | integer or nil | a shaming text in place of the content for others | nil | S4 `0` [7]–[11], `0.10` |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | **The handler.** `on_chat_message` takes six parameters (S4 `0.16`), registered as `__stub[378]` (S4 `0` [38]–[40]); the dispatcher calls it with every value after the id (S2 `0.15` [21]–[26]). It drops a line whose content is nil or `""`, or that comes while there is no local player [1]–[10], then (S4 `0.10`) reads the sender as `""` when nil, drops a blacklisted sender's line, applies the options and the template, caches the line, fires `EVENT_CHAT_MESSAGE(channel, sender, content, receiver)`, and puts a speech bubble over the sender's actor on channels 0, 2, 3 and 50 (S4 `0.5`; `0` [3]–[6]). So Fenji writes all six arguments, the last two nil. | **Observed** (bytecode) | S4; S2 `0.15` |
| 2.2 | **The player link.** `GetPlayerLink(pid, name, vocation, gid, …)` (S7 `0.20`) writes ` [link type="player" pid="%d" name="%s" vocation="%d" gid="%d"`, with a space before it [12], then any of `guildID`, `guildName`, `camp`, `gm` and `icon_data`, then `] `, with a space after. The name goes through `EscapeLinkParam` (S7 `0.0`), which writes a space, `'`, `"` and `\` as `\` and the byte in three decimal digits; `ReverseLinkParam` reads them back (S7 `0.1`). Readers take the pid, the name and the group with patterns (`pid="(%S-)"` and the like: S3 `0.3`, S5 `0.29`, S6 `0.7`). The player render (S8 `0.12`) needs the vocation, colours the name by it, draws a GM badge for a `gm` above 0, and writes `※` before the name unless `NET.IsFromSameServer(the local player's property 31, gid)`, which holds when the two are equal or either is nil (S9 `0.18`). Fenji is one server and never sets property 31, which then reads 0, so its links carry `gid="0"` and nothing optional: no badge, no guild. In the runs no name showed a "※" (§7). | **Observed** (bytecode; runs), **Inferred** (property 31 reads 0) | S7; S8 `0.12`; S9 `0.18`; D2; §7 |
| 2.3 | **How a line shows** (S6 `0.5`). On channel 1, "You tell {1}: '{2}'" with the receiver when the sender's pid is the local player's, else "{1} tells you: '{2}'" with the sender ([29], [36]); any other channel, "{1}: {2}" with the sender, unless the sender is `""` or the channel is 64, 65, 57 or 69 ([86]–[88]). A label with the channel name's first letter comes first when the channel is shown (S6 `0.4`): N for Normal, W for World and for Whisper, S for System. The line shows in each chat tab that lists the channel's index, and 53 and 66 in every tab (S5 `0.27`). The runs' lines showed so (§7). | **Observed** (bytecode; D4 for the English; runs) | S6 `0.5`, `0.4`; S5 `0.27`; D4; §7 |
| 2.4 | **Whispers.** Both ends get the same 378, with the speaker's link as the sender and the target's as the receiver (2.3 picks the wording by the sender's pid). On a whisper the listener (S5 `0.29`) drops a line whose sender has no `pid=`, sets the reply target (`SetLastSender`, S3 `0.3`, which needs pid, name and gid and skips the player's own pid) and plays a sound. | **Observed** (bytecode) | S5 `0.29`; S3 `0.3` |
| 2.5 | **Markup in a line.** The chat line is a `Label` (S6's layout), whose `SetText` (S14 `0.0`) runs a text of more than 3 bytes that holds a `[` through the tag parser (`UI_PARSE.ParseLabelTag`) [33]–[48], then `ReverseBracket` [49]–[55], then sets it as HTML [56]–[59]. The parser's text scanner (S15 `0.12`) reads a `\` before a bracket as the bracket itself [44]–[55], and starts a tag at any other `[` [56]–[66]. The HTML reads `&lt;` as `<`. So a player's line, already in that form (1.5), goes as it came, and the world refuses the markup a player's input never makes. The server's own text, such as a command's usage `tp <name> [<name>]`, goes with `&`, `<` and `>` as entities and a `\` before each bracket (`chat::say`'s `System`), and showed as written in t16-c1: "usage: tp <name> [<name>]" (§7). | **Observed** (bytecode; t16-c1) | S14 `0.0`; S15 `0.12`, `0`; §7 (T16C1) |

## 3. The channels Fenji uses

| Channel | Client name | Shown as | Fenji's use | Evidence |
|---|---|---|---|---|
| 0 | `CHANNEL_NEARBY`, Normal | label N, "sender: text", a bubble over the speaker | nearby: whoever has been told of the speaker (6), and the speaker | S16; S10 `0`; S4 `0.5` |
| 1 | `CHANNEL_PERSONAL`, Whisper | label W, "You tell …" / "… tells you" | whispers, to both ends | S16; S6 `0.5` |
| 5 | `CHANNEL_WORLD`, World | label W | everyone in the game | S16; S10 `0` |
| 64 | `CHANNEL_SYSTEM_FRAME_YELLOW`, System | label S, yellow, never a sender | the server's line to one player: a command's answer, `!invite`'s link | S10 `0` [535]; S6 `0.5` [88]; S17 |
| 66 | `CHANNEL_SYSTEM_GOROUND`, System | no label; the marquee across the top, and a line in every chat tab | `!announce`, for everyone | S10 `0` [562]; S5 `0.27` [13]; S11 `0.1` |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 3.1 | **64 for a line one player sees.** 52 (`CHANNEL_SYSTEM_NOTICE`) and 64 both have the System index (12) and a label (S10 `0` [491], [535]), and show in the tabs that list index 12. 64 never shows a sender (2.3), and the game's own shared scripts address one player with `Say(64, …)` 66 times and never with 52 (S17). So the server's own lines to one player go on 64. | **Observed** (the table, operands), **Inferred** (the choice) | S10; S6 `0.5`; S17 |
| 3.2 | **66 for announcements.** The Marquee add-on queues and scrolls every line on 66 whose content isn't empty (S11 `0.1`), and the chat box shows 66 in every tab, without a label (index 12, not shown: S10 `0` [562]; S5 `0.27`). t16-c1's announcement did both on both clients (§7). | **Observed** (bytecode; t16-c1) | S11; S10; S5; §7 (T16C1) |

## 4. Chat's properties: 389 and 396

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.1 | **Neither needs to be sent.** Both are `table` properties synced to the player alone (D2), and an unset table property reads as `{}` ([`world.md`](world.md) 4.11). | **Observed** (D2), **Inferred** | D2; world.md 4.11 |
| 4.2 | **389 `prop_sys_chatbox_setting`.** `GetChatBoxSetting` (S10 `0.1`) reads it [6] and, finding it empty (`next` is nil), uses the default settings and sends them with 422 `PROTOC2S_CHATBOX_SETTINGS_CHANGE` (S10 `0.0` [13]). So a client sends 422 for as long as the server never sets 389, at every read: 282 in t16-c1's five minutes, 33 from each client on entering and about six for each line a client showed (§7). Its chat settings start from the defaults each session. Fenji ignores 422 in M4. | **Observed** (bytecode; t16-c1) | S10 `0.1`, `0.0`; D1; §7 (T16C1) |
| 4.3 | **396 `prop_sys_emoji_infos`.** `IsEmojiActivated(id)` (S13 `0.2`) indexes it with the emoji's id [16]; empty, no emoji counts as unlocked, so the client refuses a line with a valid `#<id>` itself ("Not allowed to send emotes", D4) and sends any other. | **Observed** (bytecode) | S13; S1 `0.6`; D4 |

## 5. Refusals: 270 and the chat codes

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 5.1 | **270's handler** (S12 `0.8`) prints the code's row (S12 `0.3`): its text, formatted with any arguments, on the row's channels or, when it names none (every chat row), on 63 (S12 `0` [12]), which GameAlert shows as red header text; with the row's voice line when it has one. It fires `EVENT_SERVER_ERROR` too. A code is the sheet's `ID`, a 64-bit integer ([`lobby.md`](lobby.md) 8.2). In the runs, "Invalid chat channel" and "• You're typing too fast!" showed so (§7). | **Observed** (bytecode; runs) | S12; §7 |
| 5.2 | **The codes,** in the table below: each `ID` is the sheet's for its `literal` (D3), and each English text the client's (D4). `fenji_proto::chat::code` names them by literal without `ERR_`, as `lobby::code` does. | **Observed** (the sheet, the texts) | D3; D4 |

| Refusal | `literal` | `ID` | English text | Voice | Who refuses |
|---|---|---|---|---|---|
| `Invalid` | `ERR_CHAT_INVALID_CONTENT` | 8639093037559110139 | "The message cannot be sent" | no | the world (markup, hidden characters, an empty line); the link (no text) |
| `TooLong` | `ERR_CHAT_MAX_CHAR` | -6664778746412662761 | "The number of words in the chat exceeds the limit" | no | the world (over 140 characters); the link (over 2,048 bytes, or a whisper's name over 64) |
| `TooFast` | `ERR_CHAT_TOO_FAST` | -7815789390081211457 | "You're typing too fast!" | yes | the world |
| `NotOnline` | `ERR_CHAT_RECEIVER_OFFLINE` | 2418279538352872837 | "The person you are whispering to is not online" | yes | the world |
| `ToSelf` | `ERR_CHAT_TALK_TOSELF` | 437753046416001599 | "You cannot whisper to yourself" | no | the world |
| `Channel` | `ERR_CHAT_INVALID_CHANNEL` | 1246893794701349639 | "Invalid chat channel" | yes | the link |

## 6. What Fenji sends

| The client sends | Fenji answers | Values |
|---|---|---|
| 115 on 0 or 5 | 378 to the speaker and to whoever the world routes it to | `(378, channel, the speaker's link, the text, "", nil, nil)` |
| 115 on 1, `extra` with a pid or a name | 378 to both ends | `(378, 1, the speaker's link, the text, the target's link, nil, nil)` |
| 115 from an admin, `!` and a letter | the command's effect, and its answer as 378 on 64 | `(378, 64, "", the answer, "", nil, nil)`, the answer made to show as written (2.5) |
| `!announce text` from an admin | 378 on 66 to everyone | `(378, 66, "", text, "", nil, nil)` |
| 115 on another channel, or without text | 270 at once, from the link | `(270, ERR_CHAT_INVALID_CHANNEL)` or `(270, ERR_CHAT_INVALID_CONTENT)` |
| 115 whose text is over 2,048 bytes, or a whisper to a name over 64 bytes | 270 at once, from the link | `(270, ERR_CHAT_MAX_CHAR)` |
| a line the world refuses | 270 to the speaker | §5's codes |
| 422 | nothing | 4.2 |

**Golden vectors** are in `crates/fenji-proto/tests/chat.rs`: V16 (Lin's "hi" nearby, 80 bytes), V17 (a whisper, 146 bytes), V17b (the world channel), V18 (an announcement), V19 (a usage line), V20 (270 `ERR_CHAT_TOO_FAST`), and the client's own forms, V21 to V23, from 1.2 and 1.4. The link's mapping is in `crates/fenji-server/src/link.rs`, and the plan's Amendment D gives the protocol bot's script.

## 7. Runs

Task 16's runs (2026-10-15) ran two clients at once as Task 13's and Task 15's did ([`world.md`](world.md) §13, §16), with `run-client.sh run --clients 2`, against `fenji-server` built from the task's branch, with the watchdog armed. Since #267 the sandbox renders on the host's GPU, so the clients loaded a scene in 6 to 10 s, against 40 to 58 s on lavapipe. Each run used its own copy of Task 15's creation database: Lin (pid 1, a Fox Mage, account `tester`) and Mai (pid 2, a Spellsword, `second`), and the server's `[access] admins` named `tester`. Inputs went through the runner's own tools, `xkey.py` and `xclick.py`, on each client's nested display (`:99`, `:97`), and host shots of both displays came every one or two seconds. t16-c1 ran a build for that run alone that also logged each 115's and 378's body at trace (never committed); t16-c2 and t16-c3 ran the code of the pull request. Predictions were written before each run (M4R16).

| Run | What the run did | What happened | Watchdog | Evidence |
|---|---|---|---|---|
| t16-c1 | scene 5, Lin at the start and Mai 7.8 m up the path; nearby, world, a whisper typed and one clicked, a player's `!` line, the four commands, a team line, eight quick lines | every line reached whom chat.md §6 says, as predicted; the click on a name set the whisper target at once; the eight lines all passed (one every 0.64 s); `!kick` never left Lin's client (below) | no sign | T16C1 |
| t16-c2 | scene 3006, Lin at the Fox Mage's born point, Mai at the Lianbi Hall waypoint, 374 m apart; a nearby line out of view, a whisper to a name in lower case, fifteen quick lines, the far `!tp` and the kick | Mai's lines as predicted, 8 of the 15 accepted and 7 refused as too fast; none of Lin's inputs reached her client (below) | no sign | T16C2 |
| t16-c3 | as t16-c2 for Lin's inputs, each after a click on her window | all four of Lin's inputs arrived; the `!tp` over 374 m brought her client's loading screen for about three seconds, then she stood beside Mai and each client showed the other; "now you hear me" reached both; `!kick Mai` closed Mai's client with `error kick-by-user` | no sign | T16C3 |

**Observed:**
- **115 on the wire** (t16-c1's bodies, kept as fixtures: `crates/fenji-server/tests/fixtures/t16-c1-*.hex`). A nearby line was `04 03 73 03 00 08 0e "hello from Lin" 00`: four values, the trailing nil included (1.2). A world line had channel 5 and a team line channel 2, the same shape. A whisper typed with `/w Lin ` was `84 00 00 01 03 73 03 01 08 11 "psst Lin from Mai" 0b 00 00 00 00 08 04 "name" 08 0b "Lin@Unknown" 00`: a header counting one table, and `extra` holding the name alone with the client's `@Unknown`. A whisper set by clicking Mai's name held `pid = 2` and then `name = "Mai@Unknown"` (1.4).
- **What the clients showed.** Both chat boxes showed each line as 2.3 has it: "N [Lin] :hello from Lin", "W [Mai] :hi world from Mai", "[Mai] tells you: 'psst Lin from Mai'" to Lin and "You tell [Lin]: 'psst Lin from Mai'" to Mai, the names in their vocations' colours with no "※". A nearby line put a speech bubble over the speaker in the other's view. A player's `!kick Lin` showed as plain chat, and nobody was kicked.
- **Clicking a name.** A left click on another player's name in a line set the whisper target and focused the input ("W You tell Mai:") without a menu; a right click opens the menu (1.4).
- **The commands** (t16-c1). `!announce Restart at 20:00` scrolled a "System" marquee across the top of both clients and added a yellow line without a label to their chat boxes. `!invite for Bob` showed Lin alone "S invite link: http://127.0.0.1:7801/#invite=…", wrapped onto two lines, as plain text: readable, and not a link the client offers to open. `!tp Mai`, 1.9 m: a 9 for Lin to Mai's client, a 20 to Lin's at Mai's place, then "S Lin is with Mai"; the next tick a 6 for Lin to Mai's client, its pull, and Lin's 7 and 8. Mai's view showed Lin far down the path at 398 s and beside him at 402 s. `!tp` alone answered "S usage: tp <name> [<name>]", the brackets shown as brackets and no backslash (2.5).
- **Refusals.** The team line came back as 270 `ERR_CHAT_INVALID_CHANNEL`, and Mai's screen showed "Invalid chat channel" in red at the top. Fifteen one-letter lines 0.28 s apart (t16-c2) gave 8 accepted and 7 refused with `ERR_CHAT_TOO_FAST`, one a second after the burst of five, as the world's bucket has it, and Mai's screen showed "• You're typing too fast!". Eight lines 0.64 s apart (t16-c1) all passed.
- **A `!tp` beyond 200 m** (t16-c3, 374 m within scene 3006): a 20 to Lin's client at Mai's place, yaw 0, and "S Lin is with Mai"; 50 ms later a 6 each way, the pulls, and the 7s and 8s. Lin's client showed its loading screen for about three seconds, then stood her at the 20's place beside Mai; no 772 and no new type 3 came, and her client's log has nothing for the jump. So a far 20 runs the client's own scene teleport ([`world.md`](world.md) 16.12). Her next nearby line reached Mai, with a bubble over her in both views.
- **`!kick`** (t16-c3): Lin got "S Mai was kicked" and a 9 for Mai, who left her view. Mai's client logged the close reason `error kick-by-user`, went back to its login screen and showed "You are offline. Please log in again if you wish to resume the game."
- **Out of view** (t16-c2, 374 m apart in scene 3006): Mai's nearby line reached Mai alone, and the world found Lin from a whisper to `lin@Unknown`.
- **Scene 3006** loaded and stood both players at the game's own arrival points: Lin on the bridge at the Fox Mage's born point in Snowing Blossom Paradise, Mai at Lianbi Hall.
- **422** (4.2): 282 in t16-c1's five minutes: 33 from each client on entering, then about six per client for each line it showed. Fenji answered none, and nothing either player saw changed.
- **No chat error** in either client's log in any run: only the listener errors on entering that [`world.md`](world.md) §13 lists, and Mai's cooldown sweep.

**Inferred:**
- **Lost key presses.** In t16-c1, Lin's `!kick Mai` (450 s) and in t16-c2 all of Lin's inputs were typed by the runner on `:99` (its `keys.log`) but never reached her client: after t16-c2 the pointer sat at (1279, 84) on `:99`'s root window, and with no window manager the focus follows the pointer ([`world.md`](world.md) 15.8). The owner's mouse crossing the nested display's window is the likely cause: during t16-c1 the owner typed into Mai's client ("hello", confirmed by the owner), and Lin's and Mai's clients sent a few more lines and moves that no runner input made. t16-c3 clicked Lin's window before each of her inputs, and all four arrived.
- **The loading screen's cause.** The scripts' actor update runs the client's scene teleport when the player's position changes by 200 m or more within one update ([`world.md`](world.md) 16.12, its S3 `0.43` and S1 `0.36`): t16-c3's 374 m jump showed the loading screen, and t16-c1's 1.9 m one didn't.

**Unknown:**
- **Where a far 20 starts.** No run sent a 20 between t16-c1's 1.9 m and t16-c3's 374 m, so the 200 m line itself wasn't tried.

**The follow-ups' runs** (2026-10-17, for 0.5.4; [`world.md`](world.md) §13), two clients as Task 16's ran:
- **A `!tp` across scenes** (054-t1 to 054-t3; [`world.md`](world.md) 1.6). The mover's client loads the other scene on a 772. On its type 3, Fenji sends a 9 naming the client's own actor, then its 7 and 8, and the client makes the actor anew. The mover stands at the target's place, "S Lin is with Mai" shows in its chat box, and each client shows the other. The refusal (#280) is lifted.

## Evidence index

`re/` is git-ignored, so the script listings, caches and notes exist only on the owner's machines. `luatool` regenerates the listings from the pinned build. A script key names the disassembly `re/m1/disasm/data/<path>.luac.dis` and the decompiled `re/m1/decompiled-all/data/<path>.lua`, with the function paths cited.

| Key | Path or address | What it holds |
|---|---|---|
| S1 | `data/core/modules/chat/chat.luac` | `SendMessageToServer` (`0.8`), `CheckTransformChatMsg` (`0.7`), the emoji check (`0.6`), the hand-off of a `/` line to the local commands (`0.5`) |
| S2 | `data/core/client/net_manager.luac` | `net_manager_send` (`0.25`); the script dispatcher (`0.15`) |
| S3 | `data/interface/addons/ChatBox/ChatBoxInput.luac` | `SetLastSender` (`0.3`), the whisper target's setter (`0.4`), the input's `/` words (`0.7`), `SetSendChannel` (`0.9`), `SetPersonalChannel` (`0.10`), `extra` (`0.18`), `SendChatMessage` (`0.19`) |
| S4 | `data/core/modules/chat/chat_receive.luac` | The main chunk `0`: the bubble's channels [3]–[6], the template [7]–[11], `__stub[378]` [38]–[40]; the cache and `EVENT_CHAT_MESSAGE` (`0.4`); the bubble (`0.5`); the filter (`0.10`); `on_chat_message` (`0.16`) |
| S5 | `data/interface/addons/ChatBox/ChatBox.luac` | The tab filter (`0.27`), the chat box's listener (`0.29`) |
| S6 | `data/interface/addons/ChatBox/ChatBoxMsgLine.luac`, with its layout `ChatBoxMsgLine.xml` | The label (`0.4`), the text (`0.5`), the label's click (`0.7`), a name with its server (`0.8`), the player menu (`0.9`; Whisper `0.9.1`); in the layout, `MsgContent` is a `Label` |
| S7 | `data/core/share/link_creater.luac` | `EscapeLinkParam` (`0.0`), `ReverseLinkParam` (`0.1`), `GetPlayerLink` (`0.20`) |
| S8 | `data/core/modules/ui_parse/ui_parse_link.luac` | The player link's render (`0.12`) |
| S9 | `data/core/modules/net/net.luac` | `GetServerNameByID` (`0.17`), `IsFromSameServer` (`0.18`) |
| S10 | `data/core/modules/chat/chat_setting.luac` | The channel table in the main chunk `0` (52 at [491], 64 at [535], 66 at [562], each with the index 12), with the `/` words; `SetChatBoxSetting` (`0.0`: 422 [13]); `GetChatBoxSetting` (`0.1`: 389 [6]) |
| S11 | `data/interface/addons/Marquee/Marquee.luac` | The marquee's listener (`0.1`: 66 [1]) |
| S12 | `data/core/modules/err/err.luac` | The main chunk `0` (63 [12], 270's stub [27]); the print (`0.3`); the 270 handler (`0.8`) |
| S13 | `data/core/modules/emoji/emoji.luac` | `IsEmojiActivated` (`0.2`: 396 [16]) |
| S14 | `data/core/modules/ui/ui_label_methods.luac` | `Label.SetText` (`0.0`) |
| S15 | `data/core/modules/ui_parse/ui_parse.luac` | `EscapeHtmlChar`'s table (the main chunk `0` [55]–[59]); the tag parser's text scanner (`0.12`) |
| S16 | `data/core/share/enum_base.luac` | The `CHANNEL_*` names |
| S17 | `data/scripts/` | The shared scripts' server-side `Say(channel, …)` calls: channel 64 in 66 of them, 52 in none (counted in the decompiled text) |
| D1 | `data/caches/protocol_ver.new_cache` | The protocol table: 115 `PROTOC2S_CHAT_TRANSMIT` and 422 `PROTOC2S_CHATBOX_SETTINGS_CHANGE` (client to server), 378 `PROTOS2C_SYS_CHAT_MESSAGE` and 270 `PROTOS2C_ERROR_CODE` (server to client) |
| D2 | `data/caches/prop_tables.new_cache` | 31 `prop_base_group_id` (`int`, sync `all`, no default), 389 `prop_sys_chatbox_setting` and 396 `prop_sys_emoji_infos` (`table`, sync `self`) |
| D3 | The `errors` sheet of `data/caches/all_tables.new_cache` (`re/m3/tables/errors.jsonl`) | The chat rows: `ID`, `literal`, `channels` (empty), the voices |
| D4 | `data/caches/locale_en.new_cache` | The English texts: the codes' rows, "You tell {1}: '{2}'", "{1} tells you: '{2}'", "Whisper", "System", "Unknown", "Not allowed to send emotes" |
| X1 | FUN_140221A00 (`send`; its call to the packer at 0x140221A4D–0x140221A68), FUN_1400D54F0, FUN_1400D5280, FUN_140116240 | How `send` packs a script message: every value from the stack's index 2 to its top |
| X2 | FUN_140222E00 | An inbound type 0, unpickled whole (FUN_1400D5850) |
| L4 | `re/m4/world-survey/` | The design-time survey (its chat part, `C-chat.md`): the leads this reading checked |
| L7 | `re/m4/chat/notes.md`, with the REA bundle `re/m4/chat/rea/bundle-1.json` | This reading's notes: every REA call and its ID, the unknowns recorded in REA, the bytecode positions |
| T16C1 | `re/runs/out/t16-c1/` and `re/runs/io/t16-c1/`, with host shots in `re/m4/t16/c1/` | Task 16's first run: `io/t16-c1/server.log` (trace, with the run-only build's 115 and 378 bodies), both clients' `gujianol.log` (client 2's in `client2/`), `keys.log`, the watchdog's log; the by-hand inputs in `re/m4/t16/c1/hand.log`; the run-only patch `re/m4/run/t16-c-dump.patch` and the build's hash |
| T16C2, T16C3 | `re/runs/out/t16-c2/`, `re/runs/out/t16-c3/`, their `re/runs/io/` folders, with host shots in `re/m4/t16/c2/`, `re/m4/t16/c3/` | The same for the runs on the clean build, with each build's hash in `re/m4/run/` |
| M4R16 | `re/m4/t16/runs.md` | Task 16's predictions, written before each run, and their outcomes |

### REA Evidence IDs

Read through REA 6.3.0 with Ghidra 12.1.4 on the pinned exe, in one session on 2026-10-15, whose first call, on FUN_140222E00, returned the ID [`world.md`](world.md) gives for it. FUN_140221A00's, FUN_1400D54F0's and FUN_1400D5280's pseudocode IDs are the ones [`opcodes.md`](../opcodes.md), [`transport.md`](../transport.md) and [`payloads.md`](../payloads.md) cite, repeated here. An ID covers its call exactly as sent, so anyone can check it by repeating the call ([`../../re/README.md`](../../re/README.md#how-to-cite)).

| Function or address | Cited in | Call | Evidence ID |
|---|---|---|---|
| FUN_140222E00 | X2; 2.1 | `procedure_pseudo_code {procedure: "0x140222e00"}` | `ev_4656519e08dab96fa5ad38e228a578375ef1b30907fb075530e4d9e15b4d3d20` |
| FUN_140221A00 | X1; 1.1, 1.2 | `procedure_pseudo_code {procedure: "0x140221a00"}` | `ev_b88b93181e1a9ba5a8e782085daa916772b3fd53bebdefdfd36bcf0a682e4e7e` |
| FUN_140221A00, its instructions | X1; 1.2 | `procedure_assembly {procedure: "0x140221a00"}` | `ev_1c9884bfd1f3dacf052c46ff23ad8f8899cf5c53a33d48725061039f2198a780` |
| FUN_1400D54F0 | X1; 1.2 | `procedure_pseudo_code {procedure: "0x1400d54f0"}` | `ev_72d7da265dc773a97706666c71deea2e6186b049b477a6ffccb8e4432d1d7fef` |
| FUN_1400D5280 | X1; 1.2 | `procedure_pseudo_code {procedure: "0x1400d5280"}` | `ev_cb34a8a1d02b59013d2bc2558c3f7991686e0c612d9720eb2471f41d84a24888` |
| FUN_140116240 | X1; 1.1 | `procedure_pseudo_code {procedure: "0x140116240"}` | `ev_30d52a9689ca679911bed188214d15758d0504a066276cfdacd89c3fbd214377` |

The session also recorded two questions for the runs as REA unknowns, both answered there: how a system line's brackets show (`unk_10595bc84c92419b33f7c2178c1935f31f66c22e2742631040c4ddd7eb3352b8`; 2.5) and how often the client sends 422 (`unk_4305ebb750cb986eb36cfa0821820cf6af32d2e0168fae870e9abeac8f128fde`; 4.2).
