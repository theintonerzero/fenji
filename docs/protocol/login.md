# Login: how the client finds its server

On the Gameforge platform, the client gets its server list by downloading the URL in its `serverlist_url` setting. The login screen offers the servers in that list, and the client connects to the gate address of the one the player picks. The client reads a setting either from a `key=value` command-line argument or from an environment variable of the same name. So a run can point the client at our stub without touching any client file: pass `serverlist_url=` and a one-entry list whose gate is `127.0.0.1` (§3).

**Method.** This reading comes from the pinned build 10015727 (`bin64/gujianol.exe`, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`). It used two sources:
- the client's own scripts, disassembled and decompiled locally by `luatool` (`docs/re/m1-script-format.md`);
- read-only Ghidra queries on the exe.

The client was not run. Script evidence cites the script's engine path and its function path in the disassembly (such as `0.67`). Native evidence cites addresses in the pinned exe. Every part was written by an agent that has not read any source rejected in [`../re/sources.md`](../re/sources.md). Tags follow [`../re/spike-0.md`](../re/spike-0.md): **Observed** (read from the files), **Inferred** (our reading, including code readings), **Unknown**.

## 1. Inputs the client reads

| Input | Where it is read | What it controls | Tag | Evidence |
|---|---|---|---|---|
| Setting `serverlist_url` | Gameforge list fetch, `data/core/modules/net/net.luac` function `0.67`; Chinese gate fetch, `0.57` | The URL of the server list. Downloaded with the client's curl binding (`curl_easy_download`, 10 s timeout). When unset, the Gameforge path falls back to `file:///D://wy_debug.json` and the Chinese path to an official address. | **Observed** (constants), **Inferred** (flow) | S1 |
| Setting `gj_gates` | Connect function, `net.luac` `0.60` | A gate as `host:port`. Used on the Gameforge path only when the list has no entry at the chosen index (the zone is then `"90"`), and on the Steam path instead of the gate fetch. | **Inferred** | S1 |
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
| 2.4 | On the login screen, a Gameforge client that isn't logged in shows the server-choice frame for that list (`data/interface/addons/Login/Login.luac` `0.7`). Clicking a server stores its `server_id` as the client setting `SERVERINDEX` and calls `NET.Login(index)` (`0.30`). Other platforms call `NET.Login` directly. | **Inferred** | S3 |
| 2.5 | `NET.Login` (`net.luac` `0.60`) takes the chosen entry's gate. It resolves the host with `util_gethostbyname`, builds `host:port`, takes the zone and server name from the entry and the ticket from the Gameforge SDK, and calls `net_manager_login`. | **Inferred** | S1 |
| 2.6 | `net_manager_login` (`data/core/client/net_manager.luac` `0.20`) packs the ticket, the account fields, the client version, a machine fingerprint and the user language with `ks.pickle_pack`. It then calls `net_client_create(gate, packed, 3861871408)`, the binding spike-0 found at 0x140222970, which reads two strings and an integer. | **Inferred** | S2 |
| 2.7 | What the constant 3861871408 (0xE62F7B30) means. | **Unknown** | S2 |

## 3. Pointing the client at our stub

None of these inputs touches a client file; all are inputs the client already reads (design spec §5.3, option 1).

| Input | Value for the stub | Why |
|---|---|---|
| Command-line argument | `serverlist_url=file:///<run directory>/serverlist.json`, the path as the Wine process sees it | The Gameforge fetch reads this setting (§1); curl reads `file://` URLs, so the stub needs no HTTP server. |
| The file `serverlist.json` in the run's throwaway directory | One server: `server_id` 1, `server_name` `Fenji`, `zone_id` `"90"`, `server_gate` with `ip` `127.0.0.1` and `port` 7800 | The frame needs one entry to offer, and the gate is the stub. |
| Command-line argument (fallback) | `gj_gates=127.0.0.1:7800` | Used if the list has no entry at the chosen index. |
| The stub | `fenji-stub --listen 127.0.0.1:7800` | The gate. The port is ours to choose, because the list carries it. |

Expected port: TCP 7800 on loopback, as the list says. A literal IP needs no name resolution, though `util_gethostbyname` still runs (2.5). The same values could also come from environment variables of the same names (§1). Task 10 verifies the first route that works and records it in §6.

## 4. The German interface

The interface language comes from the Gameforge SDK's user info, read once at start-up (FUN_1401E7D10):
- `de-DE` or `german` gives `de`;
- `fr-FR` or `french` gives `fr`;
- `ru-RU` or `russian` gives `ru_RU`;
- anything else gives `en`.

With `--gf-sdk=mocked`, that user info comes from the SDK's mocked backend, so a German interface means the mock reports German. **Inferred**. Whether the mock's language can be set lies in `gameforge_api64.dll`, outside this exe. **Unknown**.

## 5. Open questions

- The exact `file://` form that the client's curl accepts under Wine, for a path in the run directory. Task 10.
- What the server must send first: the server speaks first (spike-0 4.x).
- The Oodle Network data the client expects from the server (spike-0 4.5). M2, and the wire part of the Oodle question in gate #39.
- What the `servername` file's `server_list` serves, given that the Gameforge path doesn't read it.
- Where `--server_locale` and `--net` are read (spike-0 8.37).
- The meaning of `net_client_create`'s integer 3861871408.
- Whether the mocked SDK's language is configurable.

## 6. Verification

Filled in by M1 Task 10.

## Evidence index

`re/` is git-ignored, so the script listings exist only on the owner's machines; `luatool` regenerates them from the pinned exe. `G` keys are addresses in the pinned exe.

| Key | Path or address | What it holds |
|---|---|---|
| S1 | `re/m1/disasm/data/core/modules/net/net.luac.dis` and `re/m1/decompiled-all/data/core/modules/net/net.lua` | `OnLoad` (`0.2`), the `servername` reader (`0.1`), the Chinese gate fetch (`0.57`), `NET.Login` (`0.60`), the Gameforge list fetch (`0.67`) |
| S2 | `re/m1/disasm/data/core/client/net_manager.luac.dis` | `net_manager_login` (`0.20`) |
| S3 | `re/m1/disasm/data/interface/addons/Login/Login.luac.dis` | The login screen's platform branch (`0.7`) and the server-choice click (`0.30`) |
| G1 | FUN_1400F4430, registered as `setting_get_str` by FUN_1400F4C20 | `ks.setting_get_str` |
| G2 | FUN_1400B91A0; settings tree at 0x143C42B58 | The setting lookup: tree, then provider, then `getenv` |
| G3 | FUN_1400B6840 | The command-line lookup (`key` or `key=value`) |
| G4 | FUN_14024F010 | Start-up: installs the command-line provider and enables the `getenv` fallback |
| G5 | FUN_1400B6760 | `--server-name` and `--daemon` |
| G6 | FUN_1401E7D10 | The interface language from the SDK user info |
| G7 | FUN_1401E79D0 | `gameforge_get_user_language` |
| G8 | FUN_1401E80F0 | `LanguageSetting.dat` and the voice language |
| G9 | FUN_14068FB60 | `--fs=` (m1-script-format §1) |
