# Login: how the client finds its server

On the Gameforge platform, the client gets its server list by downloading the URL in its `serverlist_url` setting. The login screen offers the servers in that list, and the client connects to the gate address of the one the player picks. The client reads a setting either from a `key=value` command-line argument or from an environment variable of the same name. So a run can point the client at our stub without touching any client file: pass `serverlist_url=` and a one-entry list whose gate is `127.0.0.1` (§3). Run that way, with a click on the frame's confirm button, the real client opened a session with fenji-stub (§6).

**Method.** This reading comes from the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`). It used two sources:
- the client's own scripts, disassembled and decompiled locally by `luatool` (`docs/re/m1-script-format.md`);
- read-only Ghidra queries on the exe.

§1–§5 come from reading alone; the client was not run for them. §6 records the runs that checked them (M1 Task 10). Script evidence cites the script's engine path and its function path in the disassembly (such as `0.67`). Native evidence cites addresses in the pinned exe. Every part was written by an agent that has not read any source rejected in [`../re/sources.md`](../re/sources.md). Tags follow [`../re/spike-0.md`](../re/spike-0.md): **Observed** (read from the files), **Inferred** (our reading, including code readings), **Unknown**.

## 1. Inputs the client reads

| Input | Where it is read | What it controls | Tag | Evidence |
|---|---|---|---|---|
| Setting `serverlist_url` | Gameforge list fetch, `data/core/modules/net/net.luac` function `0.67`; Chinese gate fetch, `0.57` | The URL of the server list. Downloaded with the client's curl binding (`curl_easy_download`, 10 s timeout). When unset, the Gameforge path falls back to `file:///D://wy_debug.json` and the Chinese path to an official address. | **Observed** (constants), **Inferred** (flow) | S1 |
| Setting `gj_gates` | Connect function, `net.luac` `0.60` | A gate as `host:port`. The Gameforge path reads it when the list has no entry at the chosen index (the zone is then `"90"`), but that branch can't connect (3.1). The scripts' Steam branch (platform type 4) would use it instead of the gate fetch, but that branch never runs in this build (M3 update: [`packets/lobby.md`](packets/lobby.md) 12.2). | **Inferred** | S1 |
| How a setting is read | `ks.setting_get_str` is FUN_1400F4430 → FUN_1400B91A0 | The lookup checks three sources in order:<br>1. a settings tree, which no code in the exe fills;<br>2. the command line, where FUN_1400B6840 returns the value of the last argument that is exactly `key` or starts with `key=` (no dashes);<br>3. the environment variable `key`, a fallback FUN_14024F010 enables at start-up. | **Inferred** | G1–G4 |
| File `servername` | `net.luac` `0.1`, run from `OnLoad` (`0.2`) | `script_system_resolve_path` locates the file. Its JSON object is loaded into the global `server_list`, keyed by integer id. The Gameforge connect path doesn't read `server_list`. This is the probe spike-0 8.32 saw. | **Inferred** | S1 |
| Gameforge SDK user info | FUN_1401E7D10, FUN_1401E79D0 | The interface language, and the language sent at login (§4). | **Inferred** | G6, G7 |
| `<root>/settings/LanguageSetting.dat` | FUN_1401E80F0 | The voice (sound) language. | **Inferred** | G8 |
| `--fs=<dir>:<data index>:<asset index>` | FUN_14068FB60 | Archive mounting ([`m1-script-format.md`](../re/m1-script-format.md) §1). Nothing to do with the server. | **Inferred** | G9 |
| `--server-name` | FUN_1400B6760 | The application instance name (default `GujianOL`), not a game server. | **Inferred** | G5 |
| `--server_locale`, `--net` (spike-0 8.37) | not in `gujianol.exe` as strings | Where these flags are read. | **Unknown** | |

## 2. The server list

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | When the scripts load, `net.luac` `OnLoad` first reads the `servername` file (§1). It then calls `TryFetchGameForgeServerList` (`0.67`), which runs only on platform type 3, Gameforge. | **Inferred** | S1 |
| 2.2 | The fetch downloads `serverlist_url` and counts it a success when curl returns 0 and the body isn't null. It then JSON-decodes the body and keeps the result, which `GetGameForgeServerLists` returns. Otherwise it logs `FAILED TO FETCH GAMEFORGE SERVER LIST!`, the error M0 saw (spike-0 8.15–8.17), and the list stays empty. | **Inferred** | S1 |
| 2.3 | The list is a JSON array of server objects. The client uses these fields: `server_id` and `server_name` in the server-choice frame; `server_gate` and `zone_id` to connect. `server_gate` is either one `{ip, port}` object or an array of them, from which the client picks one by a hash of the account id. | **Inferred** | S1, S3 |
| 2.4 | On the login screen, a Gameforge client that isn't logged in shows the server-choice frame for that list (`data/interface/addons/Login/Login.luac` `0.7`). The frame's confirm button (`ConfirmBtn`, "Bestätigen" in the German interface) runs `0.30`. That handler takes the selected entry, stores its `server_id` as the client setting `SERVERINDEX`, and calls `NET.Login(index)`. A click on an entry only selects it (§6). Other platforms call `NET.Login` directly. | **Inferred**; the button **Observed** (§6) | S3, C2 |
| 2.5 | `NET.Login` (`net.luac` `0.60`) takes the chosen entry's gate. It resolves the host with `util_gethostbyname`, builds `host:port`, takes the zone and server name from the entry and the ticket from the Gameforge SDK, and calls `net_manager_login`. | **Inferred** | S1 |
| 2.6 | `net_manager_login` (`data/core/client/net_manager.luac` `0.20`) packs the ticket, the account fields, the client version, a machine fingerprint and the user language with `ks.pickle_pack`. It then calls `net_client_create(gate, packed, 3861871408)`, the binding spike-0 found at 0x140222970, which reads two strings and an integer. | **Inferred** | S2 |
| 2.7 | What the constant 3861871408 (0xE62F7B30) means. **M2 update:** it is the hello's magic `K`. The client checks one word of the server's 64-byte hello against `(K OR 0xC0000000) XOR w1 XOR w0`, and a mismatch closes the connection with `CLOSED_VERSION`, so `K` works as a protocol version ([`transport.md`](transport.md) 2.2–2.4). Hellos built from it were accepted in every M2 run (§9 there). | **Inferred** (M1: **Unknown**) | S2; [`transport.md`](transport.md) 2.3–2.4, §9 |
| 2.8 | A second path skips the list: the launcher's. With the environment variable `gj_uinfo` set (or the setting `gj_uinfo_cmd`), `NET.Login` (`net.luac` `0.60`, pc 23–67 and 239–253) does four things. It reads `gj_gates` as the whole gate (`host:port`) and `gj_ticket` as the ticket. It splits `gj_uinfo` into six colon-separated fields: server id, zone, server name, account type, account and user id, with the first and last as numbers. It stores those, then calls `net_manager_login` with the gate, the ticket, a constant 0, the account, the user id and the first field. The zone, server name and account type stay in the scripts. On the other paths that first slot carries a per-platform constant, so what it means is open ([`payloads.md`](payloads.md), the auth payload). | **Inferred** | S1 |
| 2.9 | At start-up, `gj_uinfo` in the environment makes the client skip the Gameforge SDK altogether (FUN_14024F010). There is no SDK initialisation, and so no platform and no language from it. | **Inferred** | G10 |
| 2.10 | Run en-1 used `gj_uinfo=1:90:Fenji:0:tester:1`, `gj_gates=127.0.0.1:7800` and `gj_ticket=fenji-dev`. The client connected to fenji-stub in the same second its login stage was ready: no list, no click and no name lookup. Its interface was English (§4). | **Observed** | C4 |

## 3. Pointing the client at our stub

None of these inputs touches a client file; all are inputs the client already reads, or a player's click (design spec §5.3, option 1).

| Input | Value for the stub | Why |
|---|---|---|
| Command-line argument | `serverlist_url=file:///Z:/<path of the client copy>/serverlist.json`. Wine maps `Z:` to `/`, so on the owner's machine the URL is `file:///Z:/home/…/re/runs/client/serverlist.json`. | The Gameforge fetch reads this setting (§1). Curl reads `file://` URLs, so the stub needs no HTTP server. |
| The file `serverlist.json` at the client copy's install root, for one run (`run-client.sh --client-file`) | One server: `server_id` 1, `server_name` `Fenji`, `zone_id` `"90"`, and `server_gate` with `ip` `127.0.0.1` and `port` 7800 | The frame needs one entry to offer, and the gate is the stub. |
| A click on the server's entry in the server-choice frame (`run-client.sh --click`) | The entry's position on the nested display (§6) | With a list, a Gameforge client waits there for the player (2.4), and only the click calls `NET.Login`. |
| The stub | `fenji-stub --listen 127.0.0.1:7800` | The gate. The port is ours to choose, because the list carries it. |

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 3.1 | `gj_gates` can't stand in for the list on the Gameforge path. In `NET.Login` (`net.luac` `0.60`, pc 172–184), the code that splits `gj_gates` into a host and a port keeps only the first of the two values the pattern match returns, and leaves the port nil. So that branch never connects. The fallback that M1's plan listed is dropped. | **Inferred** | S1 |

Expected port: TCP 7800 on loopback, as the list says. A literal IP needs no name resolution, though `util_gethostbyname` still runs (2.5). The same values could also come from environment variables of the same names (§1). §6 records the runs.

Dev runs can also take the launcher's path (2.8–2.10): the environment variable `gj_uinfo=<server id>:<zone>:<server name>:<account type>:<account>:<user id>` and the settings `gj_gates=127.0.0.1:7800 gj_ticket=<anything>`. This needs no list and no click, and the interface is English. Whether players may use this path in place of the platform SDK is the owner's decision (#54).

## 4. The German interface

The interface language comes from the game language the Gameforge SDK reports, read once at start-up. FUN_1401A35B0 stores what `GameforgeApi_GetContext(0xb)`'s method at +0x28 returns (FUN_1401A32E0), and FUN_1401E7D10 maps it:
- `de-DE` or `german` gives `de`;
- `fr-FR` or `french` gives `fr`;
- `ru-RU` or `russian` gives `ru_RU`;
- anything else, an empty value included, gives `en`.

Nothing else sets the interface language. `settings/LanguageSetting.dat` (FUN_1401E80F0) sets only the voice language, `client_sound_language`. **Inferred**.

With `--gf-sdk=mocked`, the SDK's mock (`MockedApiModule` in `gameforge_api64.dll`) answers with constants. The getter in that slot returns `de-DE` (FUN_1800A2DB0), and a sibling returns `en-GB` (FUN_1800A2DA0). Neither reads a flag, an environment variable or the locale, so a mocked run is always German and no input changes it. The string constants are **Observed**. What the getters return, and that the client's call reaches the `de-DE` one, are **Inferred** from reading the code; this fits the German interface.

Two routes would give English without changing the client. Both are open, and both belong to M3's login question:
- **A real SDK backend that reports English.** The Steam backend maps Steam's language name (`english` gives `en-GB`; FUN_1800A1FE0).
- **The `gj_uinfo` environment variable.** It makes the client skip the Gameforge SDK at start-up (FUN_14024F010), which leaves the language empty and the interface English. It also changes the platform that the login path takes.

The first is **Inferred**. The second is **Observed** in run en-1 (C4).

## 5. Open questions

- What the server must send first: the server speaks first (spike-0 4.x). **Answered in M2:** the 64-byte hello ([`transport.md`](transport.md) §2).
- The Oodle Network data the client expects from the server (spike-0 4.5). M2, and the wire part of the Oodle question in gate #39. **Answered in M2:** a stored packet passes the client's decoder, so the server needs no encoder ([`transport.md`](transport.md) 5.8).
- What the `servername` file's `server_list` serves, given that the Gameforge path doesn't read it.
- Where `--server_locale` and `--net` are read (spike-0 8.37).
- The meaning of `net_client_create`'s integer 3861871408. **Answered in M2:** the hello's magic `K` (2.7).
- What the client sends on the launcher path, and whether players may use it (#54). **Partly answered in M2:** the same auth message as on the server-list path, with `gj_ticket` as the ticket ([`payloads.md`](payloads.md), The auth payload). Whether players may use the path stays with M3 (#54).

## 6. Verification

M1 Task 10 ran the pinned client twice inside `run-client.sh`. Both runs used:
- the sandbox options `--no-root --x11 99 --cpu-topology`, and the client flags `--gf-sdk=mocked --fs=data:_index:_index`;
- 240 s each;
- the §3 inputs;
- fenji-stub listening on 127.0.0.1:7800;
- the AGT driver watchdog, armed.

| Run | Change | Stub sessions | Names looked up | Ports tried | AGT watchdog | Exit |
|---|---|---|---|---|---|---|
| m1-c1 | §3's server list, no click | 0 | none | none | no sign; 3,953 resource lookups, none of type `PE` | 0 |
| m1-c2 | m1-c1 plus the player's server choice: a click on the entry "Fenji" at 150 s, then on "Bestätigen" at 156 s | 1 (port 7800, length 0, first 16 bytes: none) | `127.0.0.1` (`gethostbyname`) | 7800, once | no sign; 3,959 resource lookups, none of type `PE` | 0 |
| m1-c3 | m1-c2 repeated after its evidence was lost (see the evidence index) | 1 (port 7800, length 0, first 16 bytes: none) | `127.0.0.1` (`gethostbyname`) | 7800, once | no sign; 3,959 resource lookups, none of type `PE` | 0 |
| en-1 | After M1: the launcher's path (2.8–2.10), with `gj_uinfo`, `gj_gates` and `gj_ticket`; no list, no click | 1 (port 7800, length 0, first 16 bytes: none) | none | 7800, once | no sign; 3,959 resource lookups, none of type `PE` | 0 |

**Observed:**
- **The list loads.** The client reads the list from `serverlist_url=file:///Z:/…/serverlist.json`. From 120 s, its frame "Server auswählen" offered "Fenji" (C1). Curl under Wine reads that `file://` form.
- **No click, no connection.** With the list and no click, the client didn't connect in 240 s (C1).
- **The confirm button connects.** A click on the entry alone didn't connect. A click on "Bestätigen" did, in the same second: the client looked up `127.0.0.1` with `gethostbyname` and opened one TCP connection to 127.0.0.1:7800, and the stub logged the session (C2; repeated exactly in C3).
- **Nobody spoke.** Over the 85 s the connection lasted, neither side sent a byte. The session ended when the run's timeout stopped the client.
- **The launcher's path connects by itself.** In en-1 the client connected in the same second its login stage was ready, with an English interface, then sent nothing for the rest of the run (C4).
- **The watchdog saw nothing.** It found no sign of driver activity in either run. These are the first runs to log resource lookups (`+resource`), and none of them was of the driver's type `PE`.

**Inferred:**
- **Done-when 3 is met.** The real client opened a session with fenji-stub. It needed only inputs it already reads and a player's click; no client file was touched.
- **The server speaks first.** The client waited for the server, as [spike-0](../re/spike-0.md) 4.23–4.28 read it. What the server must send first is M2's first question.

**Unknown:**
- **How long the client waits.** Whether it gives up on a silent server, and when.
- **Whether the click is needed.** Whether a remembered choice (`SERVERINDEX`, with "Meine Auswahl merken" ticked) skips it in a later start. The runner starts every run from the same client copy, so these runs can't show it.

## Evidence index

`re/` is git-ignored, so the script listings exist only on the owner's machines; `luatool` regenerates them from the pinned exe. `G` keys are addresses in the pinned exe.

| Key | Path or address | What it holds |
|---|---|---|
| S1 | `re/m1/disasm/data/core/modules/net/net.luac.dis` and `re/m1/decompiled-all/data/core/modules/net/net.lua` | `OnLoad` (`0.2`), the `servername` reader (`0.1`), the Chinese gate fetch (`0.57`), `NET.Login` (`0.60`), the Gameforge list fetch (`0.67`) |
| S2 | `re/m1/disasm/data/core/client/net_manager.luac.dis` | `net_manager_login` (`0.20`) |
| S3 | `re/m1/disasm/data/interface/addons/Login/Login.luac.dis` | The login screen's platform branch (`0.7`) and the confirm button's handler (`0.30`) |
| G1 | FUN_1400F4430, registered as `setting_get_str` by FUN_1400F4C20 | `ks.setting_get_str` |
| G2 | FUN_1400B91A0; settings tree at 0x143C42B58 | The setting lookup: tree, then provider, then `getenv` |
| G3 | FUN_1400B6840 | The command-line lookup (`key` or `key=value`) |
| G4 | FUN_14024F010 | Start-up: installs the command-line provider and enables the `getenv` fallback |
| G5 | FUN_1400B6760 | `--server-name` and `--daemon` |
| G6 | FUN_1401E7D10 | The interface language from the SDK user info |
| G7 | FUN_1401E79D0 | `gameforge_get_user_language` |
| G8 | FUN_1401E80F0 | `LanguageSetting.dat` and the voice language |
| G9 | FUN_14068FB60 | `--fs=` (m1-script-format §1) |
| G10 | FUN_1401A35B0, FUN_1401A32E0; FUN_14024F010 | The SDK's game language and platform, stored at start-up; the `gj_uinfo` branch that skips the SDK |
| D1 | `bin64/gameforge_api64.dll` (SHA-256 `1250487ab0639ff5e16bc2042a9fdb8ffdfe1c3e1cfef77eacc2695c60d5908f`): FUN_1800A2DB0 (REA ev_56d0642c), FUN_1800A2DA0 (ev_94ea5b43), FUN_1800A1FE0 (ev_2a7e13ae), `GameforgeApi_GetContext` (ev_310cf39b) | The mock's constant languages, the Steam language map, the SDK's context lookup |
| C1 | `re/m1/connect/m1-c1.console`, `runs.md`, `re/stub-logs/m1-c1/` (empty); originally also `re/runs/out/m1-c1/` | Run m1-c1: the server list loaded, no click, no connection |
| C2 | `re/stub-logs/m1-c2/` (one session), `re/m1/connect/m1-c2.console`, `runs.md`; originally also `re/runs/out/m1-c2/` | Run m1-c2: the clicks, the lookup, the connect, the session |
| C3 | `re/runs/out/m1-c3/` (screenshots, `wine.log`, `clicks.log`, `watchdog.log`) and `re/stub-logs/m1-c3/` (one session) | Run m1-c3: m1-c2 repeated, with its full evidence |
| C4 | `re/runs/out/en-1/` (screenshots, `wine.log`, `gujianol.log`, `watchdog.log`), `re/stub-logs/en-1/` (one session) and `re/m1/connect/en-1.console` | Run en-1: the launcher's path, the English interface |

On 2026-10-12 a test draft deleted `re/runs`, with the run directories of m1-c1 and m1-c2: their screenshots and their Wine and watchdog logs ([`m1-summary.md`](../re/m1-summary.md)). Their stub logs, console output and notes survived. m1-c3 repeats m1-c2.

## 7. M3: the launcher path with a Fenji ticket

From M3 the launcher path (2.8–2.10) carries a Fenji game token, and the owner chose it for players (gate #114, option a, 2026-10-13; [`packets/lobby.md`](packets/lobby.md) §12):
- `fenji-launch` signs in to the Fenji server's account API, takes a game token and signs out.
- It starts the unmodified client with `gj_uinfo` (`<server id>:90:<server name>:0:<account>:<account id>`), `gj_gates` (the gate as dotted IPv4) and `gj_ticket` (the token) in its environment, never on its command line.
- The gate takes the token from the auth payload's first value and spends it. The account in its third value must be the token's own.

**The token** is `fenji1-` and 64 hex digits from 32 random bytes. The server keeps only its SHA-256. It works once, for 60 s, for one account (M3 spec D3).

**What it protects against.** It stops a token that leaked later being replayed, and tokens leaking from logs, since Fenji's own code never logs one. It doesn't hide the token on the wire: the transport has no confidentiality ([`transport.md`](transport.md) 3.9), so anyone on the path who sees the hello can read the auth payload. The token's single use is a race there, which the client usually wins within about 10 s. A host who needs more puts the gate on a network the players trust.

**Runs.**
- In m3-l1 the real client logged in with such a token 8.4 s after the server came up.
- In m3-l6 it did the same with no `--gf-sdk` flag at all, so the player path needs nothing else.

Both runs are in [`packets/lobby.md`](packets/lobby.md) §14, with the list, create, restart and delete runs.

