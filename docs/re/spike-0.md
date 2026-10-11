# Spike 0: pinned client triage

Static triage of the pinned SoLO client: build 10015727, `bin64/gujianol.exe` SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678` ([`pinned-build.md`](pinned-build.md)). Sections 1–7 are static. They come from the files on disk, from read-only queries to the shared Ghidra project, and from what the owner's one launch through Steam left behind. Task 7 adds an isolated runtime observation as §8, and Task 8 the conclusions.

**Method.** The 2026-10-11 triage (`re/triage/triage-report.md`) ran on the owner's Steam install, whose `gujianol.exe` has the same SHA-256, so its addresses apply to the pinned exe. Its headline facts were then re-verified on the pinned copy with a read-only, standard-library script, and a second run of the script gave the same results. They were also spot-checked independently with `sha256sum`, GNU `objdump`, `grep`, a second small reader and read-only Ghidra queries. Nothing in the client was executed, modified or extracted. This document was written by an agent that has not read any source rejected in [`sources.md`](sources.md). REA was not used for it (its auto-analysis of this exe is too slow, as [`README.md`](README.md) notes), so there are no REA Evidence IDs. Addresses are virtual addresses in the pinned exe (image base 0x140000000); "file 0x…" is a file offset.

| Tag | Meaning |
|---|---|
| **Observed** | Read from the pinned build's files by a tool. Anyone with the pinned build can reproduce it. |
| **Observed (local)** | Seen only in local evidence that the pinned build can't reproduce: the owner's Steam install, its Proton prefix and the logs of the 2026-10-11 launch. |
| **Inferred** | Our reading of observed facts, including our reading of code in Ghidra. Not confirmed at run time. |
| **Unknown** | Open. The questions for the runtime run are under "Open for runtime (Task 7)". |

Evidence keys such as `V06` or `G` resolve to files under `re/triage/` in the [Evidence index](#evidence-index). `re/` is git-ignored, so the evidence stays on the owner's machines.

In short:
- `gujianol.exe` is an ordinary, unprotected MSVC 2013 x64 build with a full exception table, named imports and plain-text strings (§2). **Inferred** from the observed layout.
- No third-party anti-cheat ships, and the exe carries an in-house kernel driver ("AGT") as a resource (**Observed**). Nothing in the exe statically extracts or loads it (**Inferred**). A dynamic path stays **Unknown** until Task 7 (§3).
- The native network layer, the opcode names and the Oodle Network set-up are readable. The client only decodes Oodle Network data, and the server speaks first (§4). **Inferred**.
- The launcher is a region picker. The game fetches a Gameforge server list itself and gets its login ticket through `gameforge_api64.dll` (§5). **Inferred**.
- Much of the game logic is LZ4-wrapped, modified Lua 5.3 bytecode inside the exe, and the archives are Oodle-compressed with no sign of encryption (§6). **Observed** and **Inferred**.
- With the servers gone, the client reached its login scene and stopped at the server-list fetch (§7). **Observed (local)**.

Where re-verification disagreed with the triage report, this document follows the re-verification:
- imports: 922 functions, of which 871 by name and 51 by ordinal (the report said 922 named imports);
- the driver is a resource of the custom named type `PE`, not `RT_RCDATA`;
- the 1 MiB high-entropy run in `.rdata` has no compiled-shader (DXBC) magic, so its content is Unknown;
- the `prop_*` count depends on the search pattern (2,487 or 2,530 distinct names). The report's 2,438-entry schema, which the design spec quotes in §4, doesn't reproduce, and the schema's real size is Unknown;
- the little-endian 32-bit length prefix, the `pickle` body and the SOCKS5 support belong to the built-in remote-debugger script, not to the game transport;
- `101.3.4.2` is part of the SHA-2 object identifiers (`2.16.840.1.101.3.4.2.x`), not an address;
- the report says disassembly of the launcher shows the game's command line, but no such output was kept, so the claim is dropped (Unknown);
- the Gameforge library's `GetAuthSessionTicket` string is its own export name. The library delay-imports `steam_api64.dll`, and the Epic Online Services DLL it can delay-load is not shipped.

## 1. Inventory, engine and middleware

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 1.1 | The pinned copy holds 210 files, 107,996,004,001 bytes, in three top-level directories: `bin64/` (55 files, 196 MB), `data/` (101 files, 107.4 GB) and `tools/` (54 files, 420 MB). | **Observed** | V00, S |
| 1.2 | By type: 100 archive parts plus `_index` (99.4% of the bytes), 73 DLLs (452 MB), 10 EXEs (138 MB), 9 `.pak` (CEF), 10 `.ini`, 2 `.xml`, 2 `.bin`, 1 `.dat`, 1 `.bat` and 1 `.vPlugin`. | **Observed** | S |
| 1.3 | Main executable `bin64/gujianol.exe`: 54,029,984 bytes, PE32+ x86-64, GUI subsystem, linker 12.0, COFF timestamp 2022-11-25 09:19:37 UTC. Version resource: FileDescription and ProductName "Swords of Legends Online", FileVersion 2.1.19.42851, ProductVersion `42851-5993a494`, LegalCopyright "(C) 2007-2021, Aurogon", empty CompanyName. PDB path `C:\runner\builds\3KH-1MUd\0\gujianol\publish\branch_i18n\pub\Shipping\GujianOL.pdb`. | **Observed** | V00, V01, S |
| 1.4 | It is a Visual Studio 2013 build (linker 12.0, imports from `MSVCR120` and `MSVCP120`) of Aurogon's 古剑奇谭网络版, from an `i18n` branch. | **Inferred** | V01, V02 |
| 1.5 | Launcher `bin64/SOLO_Launcher.exe`: PE32 (x86), "SOLO Launcher" 0.0.13.47, Gameforge, signed by Gameforge 4D GmbH through thawte at 2021-07-09 11:46:15 UTC, PDB under `F:\gitlab\runner\…\tnt\solo-launcher\`. | **Observed** | V10, V11, S |
| 1.6 | The other executables in `bin64/`: `browserhost.exe` (the CEF host), `crashpad_handler.exe` and `AurCrashSender64.exe` ("Crash Report Delivery Module" 1.4.0.2). | **Observed** | V10 |
| 1.7 | `tools/` holds two developer tools. `GujianOL_Editor/` has `AurogonAssetViewer.exe`, `AurogonEngine.dll` (3,209 named exports, 54,670 exception-table entries), `VisionEnginePlugin.vPlugin` (4,129 exports), `start.bat` and `ffmpeg.exe`. `GujianOL_MusicBox/` comes with its own .NET 4.5 installer. | **Observed** | V13, S |
| 1.8 | `AurogonEngine.dll`'s named exports could help name engine functions in `gujianol.exe` (M1). | **Inferred** | S |

Engine and middleware:

| # | Component | Version or marker | Where | Tag | Evidence |
|---|---|---|---|---|---|
| 1.9 | Vision Engine | "Vision Engine" marker, build string "WIN64 DIRECTX11 - 2014, 2, 0, 1" | `gujianol.exe`; the editor ships `VisionDX11.dll` and `VisionEnginePlugin.vPlugin` | **Observed** | V10, S |
| 1.10 | Havok, NVIDIA Blast, PopcornFX, SpeedTree | markers: Havok 21, NvBlast 146, PopcornFX 124, SpeedTree 214 | `gujianol.exe` | **Observed** | V10 |
| 1.11 | FMOD and FMOD Studio | 1.10.20 (build 107978) | `fmod64.dll`, `fmodstudio64.dll` | **Observed** | V10 |
| 1.12 | Bink 2 | 2.7h/1.300h | `bink2w64.dll`; two `BINK` sections in `gujianol.exe` | **Observed** | V10, V01 |
| 1.13 | Oodle | core 2.6.3; 66 exports, including `OodleNetwork1TCP_Encode` and `OodleNetwork1TCP_Decode` | `oo2core_6_win64.dll` | **Observed** | V10 |
| 1.14 | Vivox | 5.0.0002.31499 | `vivoxsdk.dll`, `ortp.dll` | **Observed** | V10 |
| 1.15 | Chromium Embedded Framework | 3.2623.1401, with ANGLE and a Widevine adapter 49.0.2623.110 (Chromium 49) | `libcef.dll`, `browserhost.exe` | **Observed** | V10 |
| 1.16 | libcurl | 7.66.0, over schannel (72 markers) | `gujianol.exe` | **Observed** | V10, V13 |
| 1.17 | zlib, miniz | "deflate 1.2.8", a `miniz.c` path | `gujianol.exe` | **Observed** | V10 |
| 1.18 | Dear ImGui, FreeType | version resource of FreeType 2.10.1; a `dearimgui.org` URL | `puss_imgui.dll`, `gujianol.exe` | **Observed** | V10, V09 |
| 1.19 | Lua 5.3 and C modules | "Lua 5.3", `cjson`, `cmsgpack`, `pickle_pack` | `gujianol.exe` (§6) | **Observed** | V10 |
| 1.20 | TinyCC | `tinycc_add_cobject`, `__tccmacros__` | `gujianol.exe` | **Observed** | V10, S |
| 1.21 | LibTomCrypt, by its file layout | `src\ciphers\…` with `aes.c`, `anubis.c`, `blowfish.c`, `rc5.c`, `rc6.c`, `twofish.c`, `xtea.c`; the library's name doesn't occur | `gujianol.exe` | **Inferred** | S |
| 1.22 | NVIDIA SDKs | Ansel 1.4.295, Aftermath, DLSS 2.3.3, DLISP 1.2.12, GfeSDK 1.1 | `bin64/` | **Observed** | V10 |
| 1.23 | Crash reporting | `AurCrashRpt64.dll` 1.4.0.2, `sentry.dll` 0.0.4.7 (company "aurogon"), `crashpad_handler.exe` | `bin64/` | **Observed** | V10 |
| 1.24 | Platform SDKs | `gameforge_api.dll` and `gameforge_api64.dll` 2.1.3.123, `psw_tnt.dll` and `psw_tnt64.dll`, `steam_api.dll` and `steam_api64.dll`; no Epic Online Services DLL | `bin64/` | **Observed** | V10, V09, V13 |
| 1.25 | Others | `rad_tm_win64.dll` (no version resource), D3DCompiler 43 and 47, D3DX11 43, XInput 1.3, the MSVC 2013 runtime 12.0.21005.1 | `bin64/` | **Observed** | V10 |
| 1.26 | No OpenSSL, protobuf, RakNet or ENet | 0 markers each | `gujianol.exe` | **Observed** | V10 |

## 2. Protection

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 2.1 | 13 sections: `.text` (R-X, 18.2 MB raw, entropy 6.52), `BINK` (R-X, 3 KB), `.rdata` (R--, 33.2 MB, 6.51), `.data` (RW-), `.pdata` (R--, 900,192 bytes), `.CRT`, `.tls`, `.CLT`, a second `BINK` (RW-), `.udata` (RW-, shared), `_RDATA`, `.rsrc` (R--, 6.35) and `.reloc` (discardable). No section is both writable and executable, and none has a blank name. | **Observed** | V01, S |
| 2.2 | The entry point (RVA 0xAD03AC) is in `.text`. | **Observed** | V01, S |
| 2.3 | The entry point's first 16 bytes have the shape of the usual MSVC x64 CRT start-up stub: a call, then a tail jump. | **Inferred** | V01 |
| 2.4 | 922 imported functions from 34 DLLs, 871 by name and 51 by ordinal (Winsock), and no delay imports. The big DLLs contribute many functions each (`MSVCR120` 259, `KERNEL32` 180, `MSVCP120` 126, `USER32` 89): not the one placeholder per DLL that a packer leaves. | **Observed** | V02, S |
| 2.5 | Exception directory: 75,016 `RUNTIME_FUNCTION` entries, 75,001 of them starting in `.text`. 29,558 carry chained unwind info (function fragments); 45,458 don't. The shared Ghidra project counts 51,808 functions. | **Observed** | V03, S, G |
| 2.6 | The exe has on the order of 50,000 functions: 45,458 unchained exception entries (leaf functions need none) and Ghidra's 51,808. The design spec's "about 75,000 functions" (§4) counts exception-table entries. | **Inferred** | V03, G |
| 2.7 | `DllCharacteristics` is 0x8160: ASLR (dynamic base, high-entropy addresses) and DEP, but no Control Flow Guard. | **Observed** | V01, S |
| 2.8 | Authenticode: signed by "Beijing WangYuanShengTang Entertainment Technology Co.,Ltd." through DigiCert SHA2 Assured ID Code Signing CA, signing time 2022-11-25 09:21:58 UTC, with an Entrust timestamp. The chain was not validated here. | **Observed** | V11 |
| 2.9 | No protector names (Themida, WinLicense, VMProtect, Enigma) occur in `gujianol.exe`, and it has no protector-style sections. | **Observed** | V10, V12 |
| 2.10 | `.rdata` has one high-entropy run: file 0x1203800–0x1307800 (1,040 KiB, entropy above 7.5 in 16 KiB windows). The section is read-only and not executable, and the run holds no DXBC (compiled shader) magic. | **Observed** | V01, V13 |
| 2.11 | What that run holds. | **Unknown** | V01 |
| 2.12 | `IsDebuggerPresent` is imported and referenced three times: in `__raise_securityfailure` and `failwithmessage`, as Ghidra names them, and in FUN_140AD3B70, which prints "ERROR : Unable to initialize critical section in CAtlBaseModule" when a debugger is present. `CheckRemoteDebuggerPresent` is not imported. | **Observed** | V02, TX, G |
| 2.13 | All three are runtime-library failure paths (the MSVC CRT, run-time checks and ATL), not anti-debugging. | **Inferred** | G |
| 2.14 | `.udata` is a 4-byte section shared between processes, probably an instance counter that goes with the `--allow_multi` flag (see "Open for runtime"). | **Inferred** | V01, V09 |
| 2.15 | The other code-bearing binaries look the same: `AurogonEngine.dll`, `VisionEnginePlugin.vPlugin`, `gameforge_api64.dll` (built with Control Flow Guard metadata) and `puss_imgui.dll` have standard section names, exception tables and exports. | **Observed** | S |
| 2.16 | Whether the client checks its own integrity at run time. This triage didn't look for such checks. | **Unknown** | |

Verdict: **readable**. The main executable is not packed, encrypted or virtualised. It has a standard MSVC layout, a full exception table, named imports, a PDB path and plain-text strings. **Inferred** from 2.1–2.10.

## 3. Anti-cheat and the embedded AGT kernel driver

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 3.1 | No third-party anti-cheat ships. No file or directory is named after one, and no executable under `bin64/` or `tools/` contains BattlEye, BEService, EasyAntiCheat, XIGNCODE, xhunter, nProtect, GameGuard, HackShield, EhSvc, ACE-Base, TenProtect, TP3Helper, Denuvo, mhyprot or PunkBuster. | **Observed** | V12 |
| 3.2 | The only hits are `npggNT` (2), `Vanguard` (2) and `Enigma` (1) in `libcef.dll`, and the short token `EAC` in unrelated files. | **Observed** | V12 |
| 3.3 | The `libcef.dll` hits come from Chromium's own lists of known third-party modules, not from anti-cheat code. | **Inferred** | V12 |
| 3.4 | `.rsrc` holds one resource of the named type `PE`, ID 144, language 2052 (Chinese, PRC): 23,016 bytes at file 0x32D26D0, SHA-256 `e2b025aa782b725a16e556c98946ec94d38a141a77aafc674fe31e2c55096e00`. It is the only PE image embedded in `.rsrc`. | **Observed** | V06, S |
| 3.5 | That image is a 64-bit kernel driver: PE32+ x64, native subsystem, entry point in `INIT`, COFF timestamp 2018-11-12 03:14:56 UTC, 28 imports, all from `ntoskrnl.exe`. Its PDB is `D:\GJOL_reborn\AurogonGameTrust\x64\Release\AGTDriver.pdb`. It names the devices `\Device\ProcessProtect`, `\DosDevices\ProcessProtect` and `\DosDevices\Global\ProcessProtect`, and contains the strings "AGT Driver" and "Invader". It is signed through WoSign Class 3 Code Signing CA (and its G2), signer 北京网元圣唐娱乐科技有限公司, signing time 2018-11-12. | **Observed** | V06 |
| 3.6 | Imports that matter: `ObRegisterCallbacks` and `ObUnRegisterCallbacks`, `PsLookupProcessByProcessId`, `PsGetProcessImageFileName`, `IoCreateDevice` and `IoCreateSymbolicLink`, `IoCreateFileEx`, `MmFlushImageSection`, `ZwDeleteFile`, `KdDisableDebugger` and `KdEnableDebugger`, and `KeBugCheckEx`. | **Observed** | V06 |
| 3.7 | "AGT" stands for Aurogon Game Trust. The driver protects processes by filtering handle access to them (`ObRegisterCallbacks`) and takes requests through its control device. It can also delete files, switch the kernel debugger off and on, and crash the machine on purpose. Loading it is a decision gate (ground rule 6). | **Inferred** | V06 |
| 3.8 | `gujianol.exe` imports none of `FindResource*`, `LoadResource`, `LockResource`, `SizeofResource`, `DeviceIoControl`, `CreateService*`, `OpenSCManager*`, `StartService*`, `NtLoadDriver` or `ZwLoadDriver`. The names `FindResource`, `LoadResource`, `DeviceIoControl`, `CreateService`, `StartService`, `NtLoadDriver` and `ZwLoadDriver` occur nowhere in it, in ASCII or UTF-16. The driver's own names (`ProcessProtect`, `AGTDriver`, `AurogonGameTrust`, "AGT Driver", "Invader") occur only inside the resource. | **Observed** | V02, V06, S |
| 3.9 | Two strings come close. `OpenSCManagerW` (0x142E3A190) is one of four `Advapi32.dll` names that FUN_140AD4FA0 resolves with `GetProcAddress`. That function opens the service manager with `GENERIC_READ` and the `CryptSvc` service with `SERVICE_QUERY_STATUS`, reads its status and closes both handles; it creates and starts nothing. The only device path outside the resource is the WMI namespace `\\.\root\cimv2`, next to `Win32_PNPEntity` and `VID_`/`PID_` strings. | **Observed** | V06, V13, VG, G |
| 3.10 | Both are harmless: a check that Cryptographic Services is running, and game-controller enumeration. | **Inferred** | G, V13 |
| 3.11 | The shipped exe has no static path that extracts or loads the driver. | **Inferred** | V02, V06, S, G |
| 3.12 | A dynamic path can't be ruled out statically. The exe imports `LoadLibrary*` and `GetProcAddress`, embeds a C compiler (TinyCC), and keeps its 18,587 scripts LZ4-compressed, which can split strings beyond the reach of a plain search. Task 7 checks at run time. | **Unknown** | V02, V05, V10 |
| 3.13 | In the owner's launch, no `.sys` file with an AGT, ProcessProtect or Aurogon name appeared in the Proton prefix. Its 24 `.sys` files are Wine's own drivers, written during Proton's prefix set-up at 19:32:54. | **Observed (local)** | V14, TP |
| 3.14 | The triage found no service or driver entry in the prefix's registry after the launch. The registry diff itself was not kept. | **Observed (local)** | T |
| 3.15 | Gameplay-side checks exist by name: `prop_sys_cli_cheatlog`, `prop_sys_cli_debugger`, `prop_combat_skill_whitelist`, `move_set_anticheat_fail`, `EVENT_ANTICHEAT_CAPTCHA_SHOW`, `ShowCaptcha`, `cheat_monitor`, `addons/Anticheat` and `modules/anticheat`. | **Observed** | V13 |
| 3.16 | These are server-directed: a captcha, cheat logs, a skill whitelist and movement validation. Our server drives them, and none of them stops the client from starting. | **Inferred** | V13 |
| 3.17 | The exe can list processes and connections. `EnumProcesses` is called from FUN_140245560, which also opens `%s/gujianol.log` for writing. `GetProcessImageFileNameA` (called from FUN_1401A4160 and FUN_140A9D7F0) and `GetTcpTable` (called from FUN_140A9D6A0) are imported too. | **Observed** | G, TX |
| 3.18 | FUN_140245560 is the thread that writes `gujianol.log`, and on request it snapshots the names of running processes. | **Inferred** | G |
| 3.19 | What those snapshots are for: diagnostics, a cheat-tool check, or a report to the server. | **Unknown** | G |

## 4. Network layer

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.1 | Network-related imports: Winsock 2 (`WS2_32`, 32 imports including `getaddrinfo`, `WSASocketW`, `inet_pton` and `WSAIoctl`), `WSOCK32` (4, by ordinal), `IPHLPAPI` (`GetTcpTable`, `SendARP`), `WLDAP32` (18), `CRYPT32` (13: certificate stores and chains) and CryptoAPI in `ADVAPI32` (`CryptAcquireContextW`, `CryptImportKey`, `CryptEncrypt`, `CryptGenRandom`, hashing). | **Observed** | V02 |
| 4.2 | The HTTP side is libcurl 7.66.0 over schannel. `CryptImportKey` and `CryptEncrypt` are each referenced once (0x140472319, 0x14047235E). | **Observed** | V10, TX |
| 4.3 | Those two references are in libcurl's NTLM (DES) code, not in the game transport. | **Inferred** | TD |
| 4.4 | Oodle imports: `OodleNetwork1TCP_Decode`, `OodleNetwork1TCP_Train`, `OodleNetwork1TCP_State_Size`, `OodleNetwork1TCP_State_InitAsCopy`, `OodleNetwork1_Shared_Size`, `OodleNetwork1_Shared_SetWindow`, `OodleNetwork1_SelectDictionaryFromPackets_Trials`, `OodleLZ_Compress`, `OodleLZ_Decompress` and `OodleLZ_GetCompressedBufferSizeNeeded`. `OodleNetwork1TCP_Encode` is not imported, although `oo2core_6_win64.dll` exports it. | **Observed** | V02, V10 |
| 4.5 | The client decompresses Oodle Network data from the server and doesn't compress what it sends (its send path calls no Oodle function). Our server therefore needs an Oodle Network encoder, or a way to send data that the client's decoder accepts: M2's question, with Oodle licensing as a decision gate. | **Inferred** | V02, G |
| 4.6 | Serialisers present: the Lua modules `cjson` and `cmsgpack`, and `ks.pickle_pack`/`ks.pickle_unpack`. | **Observed** | V10 |
| 4.7 | Which serialiser the game protocol's payloads use. | **Unknown** | |
| 4.8 | Client state names in `.rdata`: `KS_NET_CLIENT_ST_INIT`, `IN_AUTH`, `QUEUE`, `WORK`, `ERROR`, `CLOSED`, `CLOSED_VERSION` and `CLOSED_AUTH`. | **Observed** | V04 |
| 4.9 | Their values are 0–7 in that order; the poll function FUN_14029ADD0 handles them that way. | **Inferred** | VG, G |
| 4.10 | Opcode names, contiguous in `.rdata` from 0x14119B958 to 0x14119BE60: 31 `STC_PROTOCOL_*` (server to client) and 13 `CTS_PROTOCOL_*` (client to server), listed after this table. | **Observed** | V04, S |
| 4.11 | The enum values follow that order. `SCRIPT` and `GATE_SCRIPT` carry script-level messages, which is where most gameplay traffic goes. | **Inferred** | V04 |
| 4.12 | The number each opcode has on the wire. | **Unknown** | |
| 4.13 | Related enums: `ACTOR_TYPE_*` (12 names), `TYPE_MASK_*` (7), `BROADCAST_FILTER_*` (7), `MOVE_MODE_*` (10). | **Observed** | V13 |
| 4.14 | About 2,500 distinct `prop_*` names: 2,487 with the verify script's pattern, 2,530 with a looser one. By category: `num` 1,358, `sys` 362, `switch` 242, `combat` 157, `base` 108, `npc` 97, `move` 74, and smaller groups. | **Observed** | V04, S |
| 4.15 | These are the names of the synced properties. | **Inferred** | V04 |
| 4.16 | The size of the property schema itself; the triage's 2,438 doesn't reproduce. | **Unknown** | V04 |

`STC_PROTOCOL_*`, in table order: `SCRIPT`, `CLOSE`, `TIME_SYNC`, `TRANS_LIMIT_LV`, `DEBUG_POS`, `GM_CHANNEL`, `SYNC_ID_ENTER`, `SYNC_ENTER`, `SYNC_ENTER_FINISH`, `SYNC_LEAVE`, `SYNC_UPDATE`, `SYNC_UPDATE_FILTER`, `MOVE`, `MOVE_DIR`, `MOVE_POS`, `MOVE_COMBAT`, `MOVE_JUMP_POS`, `MOVE_STOP`, `MOVE_YAW`, `MOVE_FINISH_CHARGING`, `MOVE_SYNC_SVR`, `MOVE_BASE_PROP`, `MOVE_SYNC_PATH_FIND_TASK`, `SYNC_PROP_FORCE`, `ACTOR_HMAP_MASK_REFRESH`, `OVERALL_HMAP_MASK_REFRESH`, `MOVE_ENTER_SYNC_STATE`, `MOVE_PASSIVE`, `MOVE_SURROUND`, `MOVE_FORMULAR`, `MAX`.

`CTS_PROTOCOL_*`, in table order: `GATE_SCRIPT`, `SCRIPT`, `GM_CHANNEL`, `TIME_SYNC`, `CLIENT_ENTER`, `CLIENT_LEAVE`, `SYNC_FILTER_SET`, `QUERY_ACTOR_POS`, `QUERY_ACTOR_MOVE_CMD`, `MOVE_DIR`, `MOVE_COMBAT`, `MOVE_FINISH_CHARGE`, `MOVE_DEBUG`.

The native game transport, as the code reads. The constants are **Observed** at the addresses given; what they do is our reading.

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.17 | The net set-up code references `data/core/share/gate_dictionary.bin` (from 0x14022274E), `NetClientLua` (0x14022279D) and the error "use dict_img or init net error!". The dictionary set-up calls `OodleNetwork1_SelectDictionaryFromPackets_Trials` (0x14029A772), `OodleNetwork1_Shared_SetWindow` (0x14029A788) and `OodleNetwork1TCP_Train` (0x14029A7AC), and the receive path calls `OodleNetwork1TCP_Decode` (0x14029B133). | **Observed** | V08, TX |
| 4.18 | `gate_dictionary.bin` is not a loose file in the install. | **Observed** | V09 |
| 4.19 | It comes from the archives and seeds a shared Oodle Network dictionary. | **Inferred** | V08, V09 |
| 4.20 | Scripts create the client through the Lua binding `net_client_create` (FUN_140222970), which reads two strings and an integer. The client connects to a list of IPv4 addresses (16-byte entries, FUN_14029C030) and sends nothing on connect. | **Inferred** | VG |
| 4.21 | The server speaks first. The client waits for at least 64 bytes (check at 0x14029C38F) and validates them (constants at 0x14029C3C3–0x14029C3E3, error 0x5F5E104 on a mismatch at 0x14029C3FB). It then picks one of 27 stream variants in FUN_14029BA10, which clears 0x10C8 bytes of context, divides by 27, checks that the index is at most 26 and jumps through a table. | **Inferred** | V08, VG, G |
| 4.22 | What differs between the 27 variants. | **Unknown** | V08 |
| 4.23 | The client then enters `IN_AUTH` and queues its first message: an inner header of type 0, 16 bytes taken from the shared-dictionary object, 6 bytes from `SendARP` on the socket's own local IPv4 address (most likely the network adapter's MAC address, or a marker if `SendARP` fails), and a payload that the creator stored at ctx+0x268. | **Inferred** | VG, G |
| 4.24 | The handshake function FUN_14029C330 does call `SendARP` (thunk 0x140AD423E) and, through FUN_1400C41C0, `getsockname` (`WS2_32` ordinal 6). | **Observed** | G |
| 4.25 | What the 16 bytes and the payload contain. The login ticket (§5) is one candidate for the payload. | **Unknown** | |
| 4.26 | The server's reply decides the outcome. A 2-byte body equal to "ok" (the constant at 0x1411ADEEC) moves the client to `QUEUE`. Any other body becomes the close reason (at most 255 bytes kept), with error 0x5F5E106 and state `CLOSED_AUTH`. | **Inferred** | G |
| 4.27 | The inner message header is 4 bytes. A 24-bit length is split across byte 0 (bits 16–23), byte 3 (bits 8–15) and byte 2 (bits 0–7), and byte 1 is the message type XOR the low byte of (length >> 1). Messages go out only in state `WORK` (FUN_14029AC60). | **Inferred** | VG, G |
| 4.28 | The stream is scrambled. The receive side keeps a 16-bit key at ctx+0x1480 that steps by 0xD736 per frame (0x14029B88F). The send side keeps one at ctx+0x2548, with the constants 0x28CA (send flush, 0x14029C62A) and 0x4B05 (time-sync send, 0x14029A626), and sends in chunks of at most 0x800 bytes (0x14029C625). | **Inferred** | V08, TD |
| 4.29 | While connected, the client sends a time sync at most every 2,000 ms (FUN_14029A5B0). | **Inferred** | VG |

The remote debugger, and the addresses in the exe:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.30 | The built-in plain-text scripts in `.rdata` include a remote-debugger server with its own transport: TCP messages with a little-endian 32-bit length prefix and a `pickle` body, optional SOCKS5, and a UDP announcement to ports 3756–3766 on 127.0.0.1 or on the `--debug-remote` address. | **Observed** | TS, V09 |
| 4.31 | It starts only when a debug proxy is attached, so a normal run shouldn't use it. | **Inferred** | TS |
| 4.32 | No hostname or IP:port in `gujianol.exe` looks like a game server. The two IP:port strings sit in compressed scripts: `121.5.97.203:800` near `upgrade-api.wang…/gate?gid=105` and `curl_easy_download`, and `140.210.69.34:7804` near `ShowCaptcha`. | **Observed** | V09 |
| 4.33 | The first belongs to patch download, the second to the captcha. | **Inferred** | V09 |
| 4.34 | Hostnames in `gujianol.exe`, leaving out certificate, CRL and OCSP hosts and library documentation URLs: `upgrade-api.wang…` (the rest of the name is a compression back-reference), `gjol.wangyuan.com`, `gjolintl.wangyuan.com`, `pay.wangyuan.com`, `service.wangyuan.com`, `members.cibmall.net`, `me.cibmall.net`, `gj.cibmall.net`, `cibmall.net`, `wenjuan.feishu.cn`, `forum.solo.gameforge.com`, `solo.support.gameforge.com`, `www.youtube.com`, `code.jquery.com`, `gburl.cn`, `burl.cn`, and a truncated `https://store.s…`. | **Observed** | V09 |

## 5. Startup chain, server address and ticket

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 5.1 | `SOLO_Launcher.exe` imports no networking DLL. It imports `GetTempPathW`, `CreateFileW`, `WriteFile`, `ShellExecuteExW`, `WaitForSingleObject`, `GetExitCodeProcess`, `GetCommandLineW` and `GetCommandLineA`, `LoadLibraryW` and `GetProcAddress`. | **Observed** | S |
| 5.2 | Its strings include `solo_steam.cfg`, `gujianol.exe`, `region` and `region=` (UTF-16), `serverlist_url_`, `serverlist_url=`, `--gf_reset_region` and `--gf_no_server` (ASCII), and `https://gameforge.com/en-GB/play/solo`. An embedded HTML page of about 1.3 MB is a region picker that offers `eu` and `na` and calls the host function `gf.serverInfo`. | **Observed** | V09, V13, TS |
| 5.3 | The launcher shows the region picker, remembers the choice in `%TEMP%\solo_steam.cfg`, starts `gujianol.exe` with `ShellExecuteExW` and waits for it to exit. The picker runs in a hosted web-browser control, which could fetch remote content by itself. | **Inferred** | S, TS, TP |
| 5.4 | `gujianol.exe` contains none of `solo_steam`, `.cfg`, `serverlist`, `server_list`, `region=` or `gfclient`, in ASCII or UTF-16. | **Observed** | S |
| 5.5 | So the game probably doesn't read `solo_steam.cfg`: the file is the launcher's memory of the region. | **Inferred** | S |
| 5.6 | The command line the launcher passes to the game. The triage report says disassembly showed it, but no output was kept, so the claim is dropped. | **Unknown** | T |
| 5.7 | `gujianol.exe` imports six functions from `gameforge_api64.dll`: `GameforgeApi_LoadSdk`, `GameforgeApi_GetPlatform`, `GameforgeApi_IsRunning`, `GameforgeApi_Init`, `GameforgeApi_GetContext` and `GameforgeApi_Shutdown`. FUN_1401A3580 calls FUN_1401A34A0 with the constant 1, and FUN_1401A34A0 calls `GameforgeApi_LoadSdk` with it, then `IsRunning`, `Init` and `GetContext(0xC)`. | **Observed** | V02, G |
| 5.8 | Which SDK the value 1 selects. | **Unknown** | G |
| 5.9 | `gameforge_api64.dll` (2.1.3.123, unsigned, PDB under `tnt\gameforge-client-library`) exports 38 functions, among them `GameforgeApi_IUser_GetAuthSessionTicket`, `GameforgeApi_IUser_GetAuthCode`, `GameforgeApi_IUser_GetAccountId`, `GameforgeApi_IUser_GetNumericAccountId`, `GameforgeApi_IUser_CancelAuthTicket` and `GameforgeApi_IApps_GetCurrentGameRegion`. Its only regular imports are `KERNEL32`, `ADVAPI32`, `CRYPT32` and `SHLWAPI`. It delay-imports `steam_api64.dll` (12 functions, including `SteamAPI_Init` and `SteamInternal_CreateInterface`) and `EOSSDK-Win64-Shipping.dll` (20, including `EOS_Auth_Login` and `EOS_Auth_CopyUserAuthToken`). The EOS DLL is not shipped; `steam_api.dll` and `steam_api64.dll` are. | **Observed** | V09, V10, V11, TS |
| 5.10 | The library's strings include the flags `--gf-sdk`, `--gf-appid`, `--steam-appid`, `--gf-region`, `--mocked-appid`, `--mocked-region`, `--mailru-appid` and `--gf-sgs-enabled`; the SDK names `steam`, `mocked`, `mailru` and `EpicGames`; the Gameforge client names `GC_PIPE_NAME`, `GC_USER_NAME` and `GC_GCLAY64_PATHNAME`, and "GameCenter is not running"; the config file `gfclient.ini`, which the install doesn't have; Steam interface names such as `SteamUser019`; the keys `auth_ticket`, `account_display_name` and `external_account_id`; and an Iovation module. | **Observed** | V09, V13, TS |
| 5.11 | The library reads these flags from the process command line. On the Steam path it gets a session ticket through the `SteamUser019` interface, which needs a running Steam client. On the Gameforge path it talks to the Gameforge client over a named pipe. | **Inferred** | V09, TS |
| 5.12 | A `mocked` SDK with its own app ID and region flags exists. What it does, and whether it is a usable input that the client already accepts (design spec §5.3, option 1). | **Unknown** | V09 |
| 5.13 | Ticket bindings exist by name: `steam_get_actor_ticket`, `gameforge_get_ticket`, `gameforge_get_account*` and `wegame_*`. | **Observed** | V09 |
| 5.14 | The login scripts fetch a platform ticket and send it to the server. | **Inferred** | V09 |
| 5.15 | Which message carries the ticket: `CTS_PROTOCOL_CLIENT_ENTER`, a `GATE_SCRIPT` message, or the payload of the first native message (4.23). | **Unknown** | |
| 5.16 | The game itself fetches a server list. The message "FAILED TO FETCH … SERVER LIST!" sits in a compressed script at file 0x15CE7C7. | **Observed** | V09 |
| 5.17 | The owner's log shows the full message: "FAILED TO FETCH GAMEFORGE SERVER LIST!" (§7). | **Observed (local)** | T |
| 5.18 | Where the server-list URL comes from. The candidates are the launcher's `serverlist_url` strings, the Gameforge library, and a URL inside a compressed script. | **Unknown** | |
| 5.19 | `bin64/loader.ini` has a `[CONFIG]` section with `WANGYUAN_DNS_RECOMMEND=120.133.237.40` and `WANGYUAN_DOMAIN=authentication.wangyuan.com/`. No binary in `bin64/` names these keys in plain text. | **Observed** | V09, V13, S |
| 5.20 | `loader.ini` configures the Chinese publisher's (Wangyuan) authentication and is probably unused by the Gameforge build. | **Inferred** | S |
| 5.21 | `bin64/sentry.ini` holds only local paths: `Sentry_Base = ./sentry`, `Sentry_Attachment = ./gujianol.log`. The crash reporter `AurCrashRpt64.dll` has the configuration keys `HTTPServer`, `SMTPSetver`, `SMTPLogin` and `SMTPPassword`. Their values live in the obfuscated `bin64/AurReport*.ini` files, which were not decoded. The triage saw credentials there; they are left out here and must never be used. | **Observed** | S, V13, T |
| 5.22 | A crash may make the client try to upload a report over HTTP or SMTP. | **Inferred** | V13 |
| 5.23 | The normal start depends on two services that are now dead: the Gameforge server list (seen failing, §7) and whatever validated the ticket. The exe reached its login scene without them. | **Inferred** | T, V09 |
| 5.24 | Whether the client needs a running Steam client. | **Unknown** | V09 |

## 6. Data archives and scripts

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 6.1 | `data/` holds 100 parts, `data000`–`data099` (1,073,741,559 to 1,073,741,824 bytes each), and `_index` (5,736,039 bytes). All 100 parts start with the 8-byte magic `ZFS\0[IX]`; `_index` doesn't. | **Observed** | V07, S |
| 6.2 | Each part has the magic, a 4-byte field, then a table of 4,096 records of 32 bytes (file 0x0C–0x2000C), each with an offset and a size: 247,313 non-empty records over all parts. | **Inferred** | V07 |
| 6.3 | Under that reading, the records tile the file in only 10 of the 100 parts. | **Observed** | V07 |
| 6.4 | So the record format is only partly understood. | **Unknown** | V07 |
| 6.5 | The blob that each part's first record points to begins with a 32-byte header whose 16-bit field at +28 is 4 (79 parts) or 0 (21 parts). With 4, 76 of the 79 blobs contain the bytes `8C 06` in their first 4 KiB. With 0, 16 of the 21 payloads begin with a plain file magic: `KB2j` (Bink 2 video) 13 times, `RIFF` 3 times. | **Observed** | V07 |
| 6.6 | 4 means Oodle-compressed (`8C 06` matches an Oodle LZ block header for the Kraken codec, and the exe imports `OodleLZ_Decompress`); 0 means stored. As far as these samples show, the archives are compressed, not encrypted. | **Inferred** | V07, V02 |
| 6.7 | Whether any entry type is encrypted. | **Unknown** | |
| 6.8 | `_index` begins with what reads as the same header: raw size 25,533,513, time 2022-11-25 09:28:58 UTC (nine minutes after the exe's timestamp), a stored size equal to the file size, then 0x80000 and 4. | **Observed** | V07 |
| 6.9 | `_index` is one compressed blob of about 25.5 MB, in 512 KiB chunks. Its format is M1's work. | **Inferred** | V07 |
| 6.10 | The shipped editor's `start.bat` mounts the archives with `--fs=../data:_index:_index`. | **Observed** | V13 |
| 6.11 | The engine's own file system reads the archives, and no decryption step is visible. | **Inferred** | V13 |
| 6.12 | The Lua 5.3 runtime is present ("Lua 5.3"). The exe holds 18,587 Lua 5.3 chunk headers (`ESC Lua`, 0x53, 0x00 and the `LUAC_DATA` bytes), all in `.rdata`, each preceded by the bytes `F6 04`. | **Observed** | V05, S |
| 6.13 | Read as an LZ4 block (`F6` as the token, `04` as an extra literal-length byte), the first 34 bytes of every one of the 18,587 chunks decode to a complete header. | **Observed** | V05 |
| 6.14 | Each chunk is stored as an LZ4 block. | **Inferred** | V05 |
| 6.15 | The decoded header differs from stock Lua 5.3. After `LUAC_DATA` come four size bytes (`04 04 08 08`) where stock 5.3 has five (`04 08 04 08 08`), then the stock check values `LUAC_INT` (0x5678) and `LUAC_NUM` (370.5). | **Observed** | V05 |
| 6.16 | Which size field was dropped or merged. | **Unknown** | V05 |
| 6.17 | A 54-entry opcode-name table at 0x142EF9DD0 (file 0x2EF85D0) holds the 47 stock Lua 5.3 names in stock order, then `NOP`, `XGETTABUP`, `TRYTABUP`, `TRYLIBUP`, `XGETSELF`, `XSELF` and `TRYSELF`. | **Observed** | V05 |
| 6.18 | The VM has 7 extra opcodes, numbered 47–53. | **Inferred** | V05 |
| 6.19 | Whether the VM keeps the stock numbers for the other 47. | **Unknown** | V05 |
| 6.20 | The exe contains 20,934 distinct `.lua`/`.luac` path strings, among them `combat/skill/` (6,037), `combat/buff/` (5,826), `combat/effect/` (3,966), `combat/passive` (1,207), `combat/projectile` (196) and `combat/server/` (120). The words `IsServer` and `IsClient` occur 6,834 and 6,933 times. | **Observed** | S |
| 6.21 | Much of the combat logic ships as scripts that run on both sides and branch on client or server. How much server-side logic they carry is M1's question (design spec §12). | **Inferred** | S |
| 6.22 | A region of uncompressed script source in `.rdata` (around file 0x1191000) holds the runtime's built-in helpers, such as the remote debugger (4.30). It is written in a Lua dialect with type annotations. | **Observed** | TS |
| 6.23 | Table paths occur under `tables/item/`, `tables/player/`, `tables/quest/`, `tables/present/` and `tables/cutscene/`, and 35 distinct `.tab` names in plain text. | **Observed** | S |
| 6.24 | The tables themselves are in the archives. | **Inferred** | S |
| 6.25 | With TinyCC on board (`tinycc_add_cobject`, `__tccmacros__`, the `--tcc-obj` flag), the runtime can compile C code at run time. | **Inferred** | V10, V09, S |

## 7. The owner's launch (2026-10-11)

The owner launched the game once through Steam on the Fedora box, right after the download, from 19:32:53 to 19:33:36 AEDT, under Proton GE-Proton11-7. The logs are still in the owner's Steam install and the prefix in Steam's `compatdata`; this section uses the triage's listings and report.

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 7.1 | Writes in the install: `bin64/Log_gujianol.htm` (19:33:06), `settings/GameSetting.dat` (19:33:10), `settings/PanelPositionSetings.dat` and `bin64/gujianol.log` (19:33:36), and the empty directories `settings/`, `ExportFace/` and `ExportPrefab/`. Nothing else in the install changed. | **Observed (local)** | PB, TI |
| 7.2 | The client's log: engine start on an AMD GPU (RADV) under Proton, table cache loaded, "Load Login Scene", then the error "FAILED TO FETCH GAMEFORGE SERVER LIST!", "SETUP LOGIN STAGE DONE", a net error 5 while waiting, with the net state `NET_ST_CLOSED(5)`, and a clean shutdown. | **Observed (local)** | T |
| 7.3 | The client reached its login scene with the servers gone and stopped at the server-list fetch. `NET_ST_CLOSED` = 5 matches the position of `KS_NET_CLIENT_ST_CLOSED` (4.9). | **Inferred** | T, V04 |
| 7.4 | The prefix's timeline: Proton's set-up and the VC++ 2013 redistributable (19:32:45–19:32:52), Wine's files and drivers (19:32:54), the `wine_gecko` profile (19:32:55), DXVK's state cache (19:33:01–19:33:13), the NVIDIA GfeSDK log `gujianol_856.log` (19:33:07; per the triage, a failed initialisation on the AMD GPU), the registry flush (19:33:24) and `%TEMP%\solo_steam.cfg` (18 bytes at 19:33:36; `region=na` per the triage). | **Observed (local)** | TP, T |
| 7.5 | `solo_steam.cfg` was written by the launcher, the only binary that names it, as the game exited. 18 bytes fit `region=na` in UTF-16. | **Inferred** | TP, S |
| 7.6 | Per the triage, the run added only two registry keys: the game's install directory under `HKCU\Software\GuJian\GuJianOL` and Steam's own app key. No service or driver was registered. The registry diff was not kept. | **Observed (local)** | T |
| 7.7 | No AGT driver file appeared: no `.sys` file has an AGT, ProcessProtect or Aurogon name, and all 24 are Wine's own (3.13). | **Observed (local)** | V14 |
| 7.8 | The personal data the triage saw is left out: the Steam manifest's owner fields and the crash reporter's credentials. The triage found no account name, e-mail address or token written by this launch. | **Observed (local)** | T |

## Open for runtime (Task 7)

What the isolated run should watch for. Everything here comes from the static reading above, and none of it has been confirmed at run time.

Names to watch (DNS lookups and connects):

| Source | Names |
|---|---|
| `gujianol.exe` (4.34) | `upgrade-api.wang…` (probably `upgrade-api.wangyuan.com`, path `/gate?gid=105`), `gjol.wangyuan.com`, `gjolintl.wangyuan.com`, `pay.wangyuan.com`, `service.wangyuan.com`, `members.cibmall.net`, `me.cibmall.net`, `gj.cibmall.net`, `cibmall.net`, `wenjuan.feishu.cn`, `forum.solo.gameforge.com`, `solo.support.gameforge.com`, `www.youtube.com`, `code.jquery.com`, `gburl.cn`, `burl.cn`, `store.s…` |
| `bin64/loader.ini` (5.19) | `authentication.wangyuan.com`, and 120.133.237.40 as a recommended DNS server |
| `SOLO_Launcher.exe` (5.2) | `gameforge.com`, only if the launcher runs |
| SDKs and helpers, names not in the strings | the Gameforge server list (5.16 and 5.18; the first name to look for), Steam (`steam_api64.dll`), the Gameforge client's pipe, crash upload (Sentry, crashpad, `AurCrashSender64.exe`), CEF pages (`browserhost.exe`), Vivox, NVIDIA GfeSDK |
| Authenticode blocks | the DigiCert, Entrust, WoSign and Microsoft CRL and OCSP hosts (thawte and Symantec for the launcher), only if something checks revocation |

Addresses and ports:

| Address | Context | Note |
|---|---|---|
| `121.5.97.203:800` | patch download (4.32) | port 800 can't be bound under `--no-root`; read the attempt from Wine's log |
| `140.210.69.34:7804` | captcha (4.32) | |
| 120.133.237.40 | `loader.ini` DNS recommendation (5.19) | |
| 127.0.0.1, UDP 3756–3766 | remote-debugger announcement (4.30) | expected only with a debug proxy |
| the game server | the address list scripts hand to `net_client_create` (4.20) | port Unknown; it should come from the server list |

Flags:

| Binary | Flags |
|---|---|
| `gujianol.exe` (V09) | `--allow_multi`, `--api`, `--cd`, `--cmd`, `--console`, `--daemon`, `--debug`, `--debug-remote`, `--en`, `--event`, `--force`, `--fs`, `--generate`, `--kslogall`, `--kslogtofile`, `--mastery`, `--net`, `--packed-ui`, `--porcelain`, `--remote`, `--script`, `--server-name`, `--server_locale`, `--tcc-obj`, `--test`, `--tid`, `--unit-test`, `--use-debug-log`, `--use-flat-hmap`, `--web-console`, `--web-shared`, `--web-timeout`; in UTF-16, `--adapter`, `--console`, `--log-severity`, `--queueid`, `--timeout` |
| `gameforge_api64.dll` (5.10) | `--gf-sdk`, `--gf-appid`, `--steam-appid`, `--gf-region`, `--mocked-appid`, `--mocked-region`, `--mailru-appid`, `--gf-sgs-enabled` |
| `SOLO_Launcher.exe` (5.2) | `--gf_reset_region`, `--gf_no_server` |

Values for run 2 ("the flags `SOLO_Launcher` passes"): the launcher contains none of the library's flags, so the static evidence doesn't show what it passes (5.6). The candidates are `--steam-appid` 1418100 (the Steam AppID), and `--gf-region` `na` or `eu` (the picker's codes; the owner picked `na`). No value for `--gf-appid` was found, and the flag syntax (`--x=v` or `--x v`) is Unknown. The flags' meanings are Unknown too. Going by their names, `--server-name`, `--server_locale`, `--net`, `--tid` and `--queueid` may touch the login path, and `--kslogall`, `--kslogtofile` and `--use-debug-log` may make the log more verbose (Inferred).

Files and other inputs:
- read: `bin64/loader.ini`; `bin64/sentry.ini`; `gfclient.ini` (absent: note where the library looks for it); `%TEMP%\solo_steam.cfg` (does the game open it?); `settings/GameSetting.dat` and `settings/PanelPositionSetings.dat`; `data/_index` and `data/dataNNN`, including `gate_dictionary.bin` through the engine's file system; `steam_appid.txt` (absent);
- written: `bin64/gujianol.log`, `Log_gujianol.htm`, `settings/*.dat`, `ExportFace/`, `ExportPrefab/`, crash output (`./sentry`, crashpad), `%LOCALAPPDATA%\NVIDIA Corporation\GfeSDK\`;
- environment and pipes: `GC_PIPE_NAME`, `GC_USER_NAME`, `GC_GCLAY64_PATHNAME`, and any named-pipe open;
- DLLs: whether `steam_api64.dll` loads, which shows the SDK that `GameforgeApi_LoadSdk(1)` picked (5.7, 5.8); `psw_tnt64.dll`; `oo2core_6_win64.dll`; `libcef.dll`. `EOSSDK-Win64-Shipping.dll` is absent;
- child processes: `browserhost.exe`, `crashpad_handler.exe`, `AurCrashSender64.exe`.

Network behaviour to check (Inferred, §4):
- If the server-list fetch fails, the client may never open a game connection. The server-list request is the first thing to capture.
- After connecting to a game server, the client sends nothing until it has received at least 64 bytes. A stub that only listens will see a connection without data.
- After a valid server hello, the first client message carries 16 dictionary-derived bytes, 6 bytes from `SendARP` (on loopback, probably the failure marker) and a payload.
- UDP to 127.0.0.1:3756–3766 should appear only with a debug proxy.

AGT checks. Any sign that the driver is extracted or loaded stops the run and opens a `Decision gate` issue:
1. A new `.sys` file anywhere in the prefix, or a new file starting with `MZ` in a temp directory or the client copy (Wine's own updates to `system32` and `syswow64` aside). Compare any such file with the driver: 23,016 bytes, SHA-256 `e2b025aa782b725a16e556c98946ec94d38a141a77aafc674fe31e2c55096e00`.
2. Use of the Service Control Manager beyond the known read-only `CryptSvc` query (`OpenSCManagerW`, `OpenServiceW` on `CryptSvc`, `QueryServiceStatus`; 3.9): any `CreateServiceW`, `StartServiceW`, `NtLoadDriver` or `ZwLoadDriver`, or a new key under `HKLM\System\CurrentControlSet\Services`.
3. Any open of `\\.\ProcessProtect`, `\Device\ProcessProtect` or `\DosDevices\Global\ProcessProtect`, and any `DeviceIoControl`.
4. `FindResource` or `LoadResource` on type `PE`, ID 144. Neither is imported, so such a call would have to come through `GetProcAddress`.
5. `GetProcAddress` lookups of driver or service functions from scripts or TinyCC code.

Log lines to look for: "Load Login Scene", "FAILED TO FETCH GAMEFORGE SERVER LIST!", "SETUP LOGIN STAGE DONE" and `NET_ST_CLOSED(5)`.

## Evidence index

Paths are relative to the repo root. `re/` is git-ignored: these files exist only on the owner's machines. Every finding's row above names its keys.

| Key | Path | What it holds | Findings |
|---|---|---|---|
| V00 | `re/triage/verify/00-identity.txt` | exe SHA-256 and size; files and bytes in the pinned copy | 1.1, 1.3 |
| V01 | `re/triage/verify/01-pe-header-sections.txt` | PE header, section table with entropy, debug directory (PDB), version resource, high-entropy windows | 1.3, 1.4, 1.12, 2.1–2.3, 2.7, 2.10, 2.11, 2.14 |
| V02 | `re/triage/verify/02-imports.txt` | import table; which resource, driver, service and debugger APIs are imported | 1.4, 2.4, 2.12, 3.8, 3.11, 3.12, 4.1, 4.4, 4.5, 5.7, 6.6 |
| V03 | `re/triage/verify/03-pdata.txt` | exception-directory counts | 2.5, 2.6 |
| V04 | `re/triage/verify/04-protocol-strings.txt` | `STC_`/`CTS_PROTOCOL_*`, `KS_NET_CLIENT_ST_*` and `prop_*` names | 4.8, 4.10, 4.11, 4.14–4.16, 7.3 |
| V05 | `re/triage/verify/05-lua.txt` | Lua chunk headers, the LZ4 reading, the opcode-name table | 3.12, 6.12–6.19 |
| V06 | `re/triage/verify/06-driver-resource.txt` | the embedded driver: resource entry, header, imports, PDB, strings, signer; references to it outside the resource | 3.4–3.9, 3.11 |
| V07 | `re/triage/verify/07-archives.txt` | archive magic, record tables, first-blob headers, the `_index` header | 6.1–6.6, 6.8, 6.9 |
| V08 | `re/triage/verify/08-network-constants.txt` | constants and import-table call targets at the network-code addresses | 4.17, 4.19, 4.21, 4.22, 4.28 |
| V09 | `re/triage/verify/09-endpoints.txt` | hosts, URLs, IPv4 strings and flags per binary; the Gameforge library's imports and exports; config inputs | 1.18, 1.24, 2.14, 4.18, 4.19, 4.30, 4.32–4.34, 5.2, 5.9–5.14, 5.16, 5.19, 5.23, 5.24, 6.25 |
| V10 | `re/triage/verify/10-versions.txt` | version resources in `bin64/`, middleware markers, the Oodle version | 1.5, 1.6, 1.9–1.20, 1.22–1.26, 2.9, 3.12, 4.2, 4.4, 4.6, 5.9, 6.25 |
| V11 | `re/triage/verify/11-signers.txt` | Authenticode signer names and signing times | 1.5, 2.8, 5.9 |
| V12 | `re/triage/verify/12-anticheat-names.txt` | anti-cheat and protector names in file names and binaries | 2.9, 3.1–3.3 |
| V13 | `re/triage/verify/13-extra-checks.txt` | launcher strings, Gameforge library details, enums, entropy runs, device paths, crash-reporter keys | 1.7, 1.16, 1.24, 2.10, 3.9, 3.10, 3.15, 3.16, 4.13, 5.2, 5.10, 5.19, 5.21, 5.22, 6.10, 6.11 |
| V14 | `re/triage/verify/14-prefix-drivers.txt` | every `.sys` file in the listing of the owner's prefix, with its time | 3.13, 7.7 |
| VG | `re/triage/verify/ghidra-notes.txt` | the first verifier's read-only Ghidra reading of the network code | 3.9, 4.9, 4.20, 4.21, 4.23, 4.27, 4.29 |
| | `re/triage/verify/verify_triage.py`, `re/triage/verify/README.txt` | the script that produced V00–V13, and how it was run | V00–V13 |
| RC | `re/triage/verify-recheck/00-identity.txt` … `13-extra-checks.txt`, `README.txt` | a second run of the same script, with identical results | reproduces V00–V13 |
| S | `re/triage/verify-recheck/spot-checks.txt`, with `spot-checks.sh` and `spot_checks.py` | independent checks: `sha256sum`, `objdump`, `grep` counts, the small config files, the inventory, Lua headers, the driver image, archive magic | 1.1–1.3, 1.5, 1.7–1.9, 1.20, 1.21, 2.1, 2.2, 2.4, 2.5, 2.7, 2.15, 3.4, 3.8, 3.11, 4.10, 4.14, 5.1, 5.3–5.5, 5.19–5.21, 6.1, 6.12, 6.20, 6.21, 6.23–6.25, 7.5 |
| G | `re/triage/verify-recheck/ghidra-recheck.txt` | read-only Ghidra checks of VG, the Gameforge SDK load, the process and connection imports, and the `IsDebuggerPresent` call sites | 2.5, 2.6, 2.12, 2.13, 3.9–3.11, 3.17–3.19, 4.5, 4.9, 4.21, 4.23, 4.24, 4.26, 4.27, 5.7, 5.8 |
| T | `re/triage/triage-report.md` | the 2026-10-11 triage report | 3.14, 5.6, 5.17, 5.21, 5.23, 7.2–7.4, 7.6, 7.8 |
| TI | `re/triage/inventory-files.tsv` | listing of the Steam install after the launch, with times | 7.1 |
| TP | `re/triage/prefix-files.tsv` | listing of the Proton prefix after the launch, with times | 3.13, 5.3, 7.4, 7.5 |
| TD | `re/triage/disasm/` | the triage's disassembly of the network code and of libcurl's NTLM path | 4.3, 4.28 |
| TX | `re/triage/xrefs-gujianol-net.txt` | references to network imports and strings | 2.12, 3.17, 4.2, 4.17 |
| TS | `re/triage/strings/` | string dumps: `SOLO_Launcher.*`, `gameforge_api64.*`, `gujianol-builtin-lua-region.txt` | 4.30, 4.31, 5.2, 5.3, 5.9–5.11, 6.22 |
| TR | `re/triage/rea-attempt.txt` | the triage's one REA session: the exe opened, no analysis, no evidence bundle | Method |
| PB | [`pinned-build.md`](pinned-build.md) | the pinned build, and the files the launch created | 7.1 |

REA Evidence IDs: none. REA was not used for this write-up, and the triage's one REA session recorded no evidence (TR).
