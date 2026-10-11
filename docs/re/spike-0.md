# Spike 0: pinned client triage

Static triage of the pinned SoLO client: build 10015727, `bin64/gujianol.exe` SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678` ([`pinned-build.md`](pinned-build.md)). Sections 1–7 are static. They come from the files on disk, from read-only queries to the shared Ghidra project, and from what the owner's one launch through Steam left behind. Task 7 adds an isolated runtime observation as §8, and Task 8 the conclusions.

**Method.** The 2026-10-11 triage (`re/triage/triage-report.md`) ran on the owner's Steam install, whose `gujianol.exe` has the same SHA-256, so its addresses apply to the pinned exe. Its headline facts were then re-verified on the pinned copy with a read-only, standard-library script (VS), and a second run of the script gave the same results (RC). They were also spot-checked independently with `sha256sum`, GNU `objdump`, `grep`, a second small reader and read-only Ghidra queries. That re-verification and the write-up of §1–§7 executed, modified and extracted nothing from the client; the sandboxed runs of the unmodified client are in §8. The original triage did extract the embedded driver and its certificate to a scratch directory to parse them (`re/triage/pe-gujianol-embedded-PE-resource.txt`). Every part of this document was written by an agent that has not read any source rejected in [`sources.md`](sources.md). REA was tried, but its `binary_session` status output was too large to read in this setup, so no REA analysis ran and there are no REA Evidence IDs. Addresses are virtual addresses in the pinned exe (image base 0x140000000); "file 0x…" is a file offset.

| Tag | Meaning |
|---|---|
| **Observed** | Read from the pinned build's files by a tool. Anyone with the pinned build can reproduce it. |
| **Observed (local)** | Seen only in local evidence that the pinned build can't reproduce: the owner's Steam install, its Proton prefix and the logs of the 2026-10-11 launch. |
| **Inferred** | Our reading of observed facts, including our reading of code in Ghidra. Not confirmed at run time. |
| **Unknown** | Open. The questions for the runtime run are under "Open for runtime (Task 7)". |

Evidence keys such as `V06` or `G` resolve to files under `re/triage/`, and §8's keys such as `R8` to files under `re/spike/` and `re/stub-logs/`, in the [Evidence index](#evidence-index). `re/` is git-ignored, so the evidence stays on the owner's machines.

In short:
- `gujianol.exe` is an ordinary, unprotected MSVC 2013 x64 build with a full exception table, named imports and plain-text strings (§2). **Inferred** from the observed layout.
- No third-party anti-cheat ships, and the exe carries an in-house kernel driver ("AGT") as a resource (**Observed**). Nothing in the exe statically extracts or loads it (**Inferred**). A dynamic path stays **Unknown** until Task 7 (§3).
- The native network layer, the opcode names and the Oodle Network set-up are readable. The client only decodes Oodle Network data, and the server speaks first (§4). **Inferred**.
- The launcher is a region picker. The game fetches a Gameforge server list itself and gets its login ticket through `gameforge_api64.dll` (§5). **Inferred**.
- Much of the game logic is LZ4-wrapped, modified Lua 5.3 bytecode inside the exe, and the archives are Oodle-compressed with no sign of encryption (§6). **Observed** and **Inferred**.
- With the servers gone, the client's log shows its login scene and then a failed server-list fetch (**Observed (local)**), which is where the client stopped (**Inferred**, §7).
- In the isolated sandbox, with the library's mocked platform backend (a dev-only test path, gate #11), the archive argument `--fs=data:_index:_index` and a CPU topology for Wine, the unmodified client reaches its login scene. It resolves no name through Winsock, connects nowhere and sends nothing, and none of runs 1–9 showed any sign of the AGT driver (§8). **Observed**.
- [Conclusions](#conclusions): the doc recommends proceeding to M1; the owner decides. The evidence favours inputs the client already accepts (design spec §5.3, option 1). What the client sends first, and where it gets its server address, stay Unknown at run time. The one decision gate so far, #11, is closed ([Decision gates](#decision-gates)).

Where re-verification disagreed with the triage report, this document follows the re-verification:
- imports: 922 functions, of which 871 by name and 51 by ordinal (the report said 922 named imports);
- the driver is a resource of the custom named type `PE`, not `RT_RCDATA`;
- the 1 MiB high-entropy run in `.rdata` has no compiled-shader (DXBC) magic, so its content is Unknown;
- the `prop_*` count depends on the search pattern (2,487 or 2,530 distinct names). The report's 2,438-entry schema, which the design spec quoted in §4 until M0 corrected it, doesn't reproduce, and the schema's real size is Unknown;
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
| 1.7 | `tools/` holds two developer tools. `GujianOL_Editor/` has `AurogonAssetViewer.exe`, `AurogonEngine.dll` (3,209 named exports, 54,670 exception-table entries), `VisionEnginePlugin.vPlugin` (4,129 exports), `start.bat` and `ffmpeg.exe`. `GujianOL_MusicBox/` comes with its own .NET 4.5 installer. | **Observed** | V12, V13, S |
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
| 2.4 | 922 imported functions from 34 DLLs, 871 by name and 51 by ordinal (`WS2_32` 25, `WLDAP32` 18, `WSOCK32` 4, `OLEAUT32` 3, `DSOUND` 1), and no delay imports. The big DLLs contribute many functions each (`MSVCR120` 259, `KERNEL32` 180, `MSVCP120` 126, `USER32` 89): not the one placeholder per DLL that a packer leaves. | **Observed** | V02, S |
| 2.5 | Exception directory: 75,016 `RUNTIME_FUNCTION` entries, 75,001 of them starting in `.text`. 29,558 carry chained unwind info (function fragments); 45,458 don't. The shared Ghidra project counts 51,808 functions. | **Observed** | V03, S, G |
| 2.6 | The exe has on the order of 50,000 functions: 45,458 unchained exception entries (leaf functions need none) and Ghidra's 51,808. The design spec's "about 75,000 functions" (§4, corrected in M0) counted exception-table entries. | **Inferred** | V03, G |
| 2.7 | `DllCharacteristics` is 0x8160: ASLR (dynamic base, high-entropy addresses) and DEP, but no Control Flow Guard. | **Observed** | V01, S |
| 2.8 | Authenticode: signed by "Beijing WangYuanShengTang Entertainment Technology Co.,Ltd." through DigiCert SHA2 Assured ID Code Signing CA, signing time 2022-11-25 09:21:58 UTC, with an Entrust timestamp. The chain was not validated here. | **Observed** | V11 |
| 2.9 | No protector names (Themida, WinLicense, VMProtect, Enigma) occur in `gujianol.exe`, and it has no protector-style sections. | **Observed** | V10, V12 |
| 2.10 | `.rdata` has one high-entropy run: file 0x1203800–0x1307800 (1,040 KiB, entropy above 7.5 in 16 KiB windows). The section is read-only and not executable, and the run holds no DXBC (compiled shader) magic. | **Observed** | V01, V13 |
| 2.11 | What that run holds. | **Unknown** | V01 |
| 2.12 | `IsDebuggerPresent` is imported and referenced three times: in `__raise_securityfailure` and `failwithmessage`, as Ghidra names them, and in FUN_140AD3B70, which also references the string "ERROR : Unable to initialize critical section in CAtlBaseModule". `CheckRemoteDebuggerPresent` is not imported. | **Observed** | V02, TX, G |
| 2.13 | FUN_140AD3B70 is ATL's `CAtlBaseModule` constructor: if critical-section set-up fails and a debugger is present, it prints that message. All three references are runtime-library failure paths (the MSVC CRT, run-time checks and ATL), not anti-debugging. | **Inferred** | G |
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
| 3.9 | Two strings come close. `OpenSCManagerW` (0x142E3A190) is one of four service-manager names (with `OpenServiceW`, `QueryServiceStatus` and `CloseServiceHandle`), each referenced only from FUN_140AD4FA0, which also references the strings `Advapi32.dll` and `CryptSvc`. `Advapi32.dll` is also referenced from FUN_140AD65F0, FUN_140AD6F60 and FUN_140AD76A0, and all four functions read and write the global 0x143BEA8C0. Of them, only FUN_140AD6F60 references "*** WARNING - PE SIGNATURE VERIFICATION FAILED !!!", "*** Signature override detected..." and the registry keys `SYSTEM\ControlSet001\Services\nvlddmkm` and `SOFTWARE\NVIDIA Corporation\Global`. The only device path outside the resource is the WMI namespace `\\.\root\cimv2`, next to `Win32_PNPEntity` and `VID_`/`PID_` strings. | **Observed** | V06, V13, G |
| 3.10 | Both are harmless. FUN_140AD4FA0 resolves the four functions from `Advapi32.dll` with `GetProcAddress`, opens the service manager with `GENERIC_READ` and the `CryptSvc` service with `SERVICE_QUERY_STATUS`, reads the service's status and closes both handles: a read-only check that Cryptographic Services is running, which creates and starts nothing. The global 0x143BEA8C0 is the `Advapi32.dll` handle that FUN_140AD4FA0 and the three other functions cache and share, so they form one module: a DLL-signature check (FUN_140AD6F60) that reads NVIDIA's registry keys for an override, most likely NVIDIA SDK code. The WMI strings belong to game-controller enumeration. | **Inferred** | VG, G, V13 |
| 3.11 | The shipped exe has no static path that extracts or loads the driver. | **Inferred** | V02, V06, S, G |
| 3.12 | A dynamic path can't be ruled out statically. The exe imports `LoadLibrary*` and `GetProcAddress`, embeds a C compiler (TinyCC), and keeps its 18,587 scripts LZ4-compressed, which can split strings beyond the reach of a plain search. Task 7 checks at run time. | **Unknown** | V02, V05, V10 |
| 3.13 | In the owner's launch, no `.sys` file with an AGT, ProcessProtect or Aurogon name appeared in the Proton prefix. Its 24 `.sys` files are Wine's own drivers, written at 19:32:54 as part of Proton's prefix set-up (7.4). | **Observed (local)** | V14, TP |
| 3.14 | The triage found no service or driver entry in the prefix's registry after the launch. The registry diff itself was not kept. | **Observed (local)** | T |
| 3.15 | Gameplay-side checks exist by name: `prop_sys_cli_cheatlog`, `prop_sys_cli_debugger`, `prop_combat_skill_whitelist`, `move_set_anticheat_fail`, `EVENT_ANTICHEAT_CAPTCHA_SHOW`, `ShowCaptcha`, `cheat_monitor`, `addons/Anticheat` and `modules/anticheat`. | **Observed** | V13 |
| 3.16 | These are server-directed: a captcha, cheat logs, a skill whitelist and movement validation. Our server drives them, and none of them stops the client from starting. | **Inferred** | V13 |
| 3.17 | The exe can list processes and connections. `EnumProcesses` is called from FUN_140245560, which also references the strings `%s/gujianol.log` and `w`. `GetProcessImageFileNameA` (called from FUN_1401A4160 and FUN_140A9D7F0) and `GetTcpTable` (called from FUN_140A9D6A0) are imported too. | **Observed** | G, TX |
| 3.18 | FUN_140245560 is the thread that writes `gujianol.log`: it opens the log for writing, and on request it snapshots the names of running processes. | **Inferred** | G |
| 3.19 | What those snapshots are for: diagnostics, a cheat-tool check, or a report to the server. | **Unknown** | G |

## 4. Network layer

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.1 | Network-related imports: Winsock 2 (`WS2_32`, 32 imports including `getaddrinfo`, `WSASocketW`, `inet_pton` and `WSAIoctl`), `WSOCK32` (4, by ordinal), `IPHLPAPI` (`GetTcpTable`, `SendARP`), `WLDAP32` (18), `CRYPT32` (13: certificate stores and chains) and CryptoAPI in `ADVAPI32` (`CryptAcquireContextW`, `CryptImportKey`, `CryptEncrypt`, `CryptGenRandom`, hashing). | **Observed** | V02 |
| 4.2 | The HTTP side is libcurl 7.66.0 over schannel. `CryptImportKey` and `CryptEncrypt` are each referenced once, at 0x14047231A and 0x14047235F, both in FUN_140472260. | **Observed** | V10, G |
| 4.3 | Those two references are in libcurl's NTLM (DES) code, not in the game transport. | **Inferred** | TD |
| 4.4 | Oodle imports: `OodleNetwork1TCP_Decode`, `OodleNetwork1TCP_Train`, `OodleNetwork1TCP_State_Size`, `OodleNetwork1TCP_State_InitAsCopy`, `OodleNetwork1_Shared_Size`, `OodleNetwork1_Shared_SetWindow`, `OodleNetwork1_SelectDictionaryFromPackets_Trials`, `OodleLZ_Compress`, `OodleLZ_Decompress` and `OodleLZ_GetCompressedBufferSizeNeeded`. `OodleNetwork1TCP_Encode` is not imported, although `oo2core_6_win64.dll` exports it. | **Observed** | V02, V10 |
| 4.5 | The client decodes Oodle Network data from the server but has no Oodle Network encoder: `OodleNetwork1TCP_Encode` isn't imported, and the native send path (FUN_14029C600) calls no Oodle function. Our server therefore needs an Oodle Network encoder, or a way to send data that the client's decoder accepts: M2's question, with Oodle licensing as a decision gate. | **Inferred** | V02, G |
| 4.6 | `OodleLZ_Compress` is referenced from four wrappers (FUN_14024E0C0, FUN_14024E160, FUN_14024E200, FUN_14068F5A0) and `OodleLZ_Decompress` from one (FUN_14024E060). None of them has a direct caller: each is referenced only as data, from its exception-table entry and from code at 0x14068EFE4–0x14068F06C. | **Observed** | G |
| 4.7 | What that code does with the wrappers (presumably registers them as compression callbacks), what they compress, and whether any outgoing traffic passes through them. | **Unknown** | G |
| 4.8 | Serialisers present: the Lua modules `cjson` and `cmsgpack`, and `ks.pickle_pack`/`ks.pickle_unpack`. | **Observed** | V10 |
| 4.9 | Which serialiser the game protocol's payloads use. | **Unknown** | |
| 4.10 | Client state names in `.rdata`: `KS_NET_CLIENT_ST_INIT`, `IN_AUTH`, `QUEUE`, `WORK`, `ERROR`, `CLOSED`, `CLOSED_VERSION` and `CLOSED_AUTH`. | **Observed** | V04 |
| 4.11 | Their values are 0–7 in that order; the poll function FUN_14029ADD0 handles them that way. | **Inferred** | VG, G |
| 4.12 | Opcode names, contiguous in `.rdata` from 0x14119B958 to 0x14119BE60: 31 `STC_PROTOCOL_*` names (server to client: 30 opcodes plus the `MAX` sentinel) and 13 `CTS_PROTOCOL_*` names (client to server, with no `MAX` entry), listed after this table. | **Observed** | V04, S |
| 4.13 | The enum values follow that order. `SCRIPT` and `GATE_SCRIPT` carry script-level messages, which is where most gameplay traffic goes. | **Inferred** | V04 |
| 4.14 | The number each opcode has on the wire. | **Unknown** | |
| 4.15 | Related enums: `ACTOR_TYPE_*` (12 names), `TYPE_MASK_*` (7), `BROADCAST_FILTER_*` (7), `MOVE_MODE_*` (10). | **Observed** | V13 |
| 4.16 | About 2,500 distinct `prop_*` names: 2,487 with the verify script's pattern, 2,530 with a looser one. By category: `num` 1,358, `sys` 362, `switch` 242, `combat` 157, `base` 108, `npc` 97, `move` 74, and smaller groups. | **Observed** | V04, S |
| 4.17 | These are the names of the synced properties. | **Inferred** | V04 |
| 4.18 | The size of the property schema itself; the triage's 2,438 doesn't reproduce. | **Unknown** | V04 |

`STC_PROTOCOL_*`, in table order: `SCRIPT`, `CLOSE`, `TIME_SYNC`, `TRANS_LIMIT_LV`, `DEBUG_POS`, `GM_CHANNEL`, `SYNC_ID_ENTER`, `SYNC_ENTER`, `SYNC_ENTER_FINISH`, `SYNC_LEAVE`, `SYNC_UPDATE`, `SYNC_UPDATE_FILTER`, `MOVE`, `MOVE_DIR`, `MOVE_POS`, `MOVE_COMBAT`, `MOVE_JUMP_POS`, `MOVE_STOP`, `MOVE_YAW`, `MOVE_FINISH_CHARGING`, `MOVE_SYNC_SVR`, `MOVE_BASE_PROP`, `MOVE_SYNC_PATH_FIND_TASK`, `SYNC_PROP_FORCE`, `ACTOR_HMAP_MASK_REFRESH`, `OVERALL_HMAP_MASK_REFRESH`, `MOVE_ENTER_SYNC_STATE`, `MOVE_PASSIVE`, `MOVE_SURROUND`, `MOVE_FORMULAR`, `MAX`.

`CTS_PROTOCOL_*`, in table order: `GATE_SCRIPT`, `SCRIPT`, `GM_CHANNEL`, `TIME_SYNC`, `CLIENT_ENTER`, `CLIENT_LEAVE`, `SYNC_FILTER_SET`, `QUERY_ACTOR_POS`, `QUERY_ACTOR_MOVE_CMD`, `MOVE_DIR`, `MOVE_COMBAT`, `MOVE_FINISH_CHARGE`, `MOVE_DEBUG`.

The native game transport, as the code reads. The constants are **Observed** at the addresses given; what they do is our reading.

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.19 | The net set-up code references `data/core/share/gate_dictionary.bin` (from 0x14022274E), `NetClientLua` (0x14022279D) and the error "use dict_img or init net error!". The dictionary set-up calls `OodleNetwork1_SelectDictionaryFromPackets_Trials` (0x14029A772), `OodleNetwork1_Shared_SetWindow` (0x14029A788) and `OodleNetwork1TCP_Train` (0x14029A7AC), and the receive path calls `OodleNetwork1TCP_Decode` (0x14029B133). | **Observed** | V08, TX |
| 4.20 | `gate_dictionary.bin` is not a loose file in the install. | **Observed** | V09 |
| 4.21 | It comes from the archives and seeds a shared Oodle Network dictionary. | **Inferred** | V08, V09 |
| 4.22 | Scripts create the client through the Lua binding `net_client_create` (FUN_140222970), which reads two strings and an integer. The client connects to a list of IPv4 addresses (16-byte entries, FUN_14029C030) and sends nothing on connect. | **Inferred** | VG |
| 4.23 | The server speaks first. The client waits for at least 64 bytes (check at 0x14029C38F) and validates them (constants at 0x14029C3C3–0x14029C3E3, error 0x5F5E104 on a mismatch at 0x14029C3FB). It then picks one of 27 stream variants in FUN_14029BA10, which clears 0x10C8 bytes of context, divides by 27, checks that the index is at most 26 and jumps through a table. | **Inferred** | V08, VG, G |
| 4.24 | What differs between the 27 variants. | **Unknown** | V08 |
| 4.25 | The client then enters `IN_AUTH` and queues its first message: an inner header of type 0, 16 bytes taken from the shared-dictionary object, 6 bytes from `SendARP` on the socket's own local IPv4 address (most likely the network adapter's MAC address, or a marker if `SendARP` fails), and a payload that the creator stored at ctx+0x268. | **Inferred** | VG, G |
| 4.26 | The handshake function FUN_14029C330 does call `SendARP` (thunk 0x140AD423E) and, through FUN_1400C41C0, `getsockname` (`WS2_32` ordinal 6). | **Observed** | G |
| 4.27 | What the 16 bytes and the payload contain. The login ticket (§5) is one candidate for the payload. | **Unknown** | |
| 4.28 | The server's reply decides the outcome. A 2-byte body equal to "ok" (the constant at 0x1411ADEEC) moves the client to `QUEUE`. Any other body becomes the close reason (at most 255 bytes kept), with error 0x5F5E106 and state `CLOSED_AUTH`. | **Inferred** | G |
| 4.29 | The inner message header is 4 bytes. A 24-bit length is split across byte 0 (bits 16–23), byte 3 (bits 8–15) and byte 2 (bits 0–7), and byte 1 is the message type XOR the low byte of (length >> 1). Messages go out only in state `WORK` (FUN_14029AC60). | **Inferred** | VG, G |
| 4.30 | The stream is scrambled. The receive side keeps a 16-bit key at ctx+0x1480 that steps by 0xD736 per frame (0x14029B88F). The send side keeps one at ctx+0x2548, with the constants 0x28CA (send flush, 0x14029C62A) and 0x4B05 (time-sync send, 0x14029A626), and sends in chunks of at most 0x800 bytes (0x14029C625). | **Inferred** | V08, TD |
| 4.31 | While connected, the client sends a time sync at most every 2,000 ms (FUN_14029A5B0). | **Inferred** | VG |

The remote debugger, and the addresses in the exe:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 4.32 | The built-in plain-text scripts in `.rdata` include a remote-debugger server with its own transport: TCP messages with a little-endian 32-bit length prefix and a `pickle` body, optional SOCKS5, and a UDP announcement to ports 3756–3766 on 127.0.0.1 or on the `--debug-remote` address. | **Observed** | TS, V09 |
| 4.33 | It starts only when a debug proxy is attached, so a normal run shouldn't use it. | **Inferred** | TS |
| 4.34 | `gujianol.exe` holds two IP:port strings, both in compressed scripts: `121.5.97.203:800` near `upgrade-api.wang…/gate?gid=105` and `curl_easy_download`, and `140.210.69.34:7804` near `ShowCaptcha`. | **Observed** | V09 |
| 4.35 | The first belongs to patch download and the second to the captcha. No hostname or address in the exe looks like a game server's. | **Inferred** | V09 |
| 4.36 | Hostnames in `gujianol.exe`, leaving out certificate, CRL and OCSP hosts and library documentation URLs: `upgrade-api.wang…` (the rest of the name is a compression back-reference), `gjol.wangyuan.com`, `gjolintl.wangyuan.com`, `pay.wangyuan.com`, `service.wangyuan.com`, `members.cibmall.net`, `me.cibmall.net`, `gj.cibmall.net`, `cibmall.net`, `wenjuan.feishu.cn`, `forum.solo.gameforge.com`, `solo.support.gameforge.com`, `www.youtube.com`, `code.jquery.com`, `gburl.cn`, `burl.cn`, and a truncated `https://store.s…`. | **Observed** | V09 |

## 5. Startup chain, server address and ticket

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 5.1 | `SOLO_Launcher.exe` imports no networking DLL. It imports `GetTempPathW`, `CreateFileW`, `WriteFile`, `ShellExecuteExW`, `WaitForSingleObject`, `GetExitCodeProcess`, `GetCommandLineW` and `GetCommandLineA`, `LoadLibraryW` and `GetProcAddress`. | **Observed** | S |
| 5.2 | Its strings include `solo_steam.cfg`, `gujianol.exe`, `region` and `region=` (UTF-16), `serverlist_url_`, `serverlist_url=`, `--gf_reset_region` and `--gf_no_server` (ASCII), and `https://gameforge.com/en-GB/play/solo`. An embedded HTML page of about 1.3 MB is a region picker that offers `eu` and `na` and calls the host function `gf.serverInfo`. | **Observed** | V09, V13, TS |
| 5.3 | The launcher shows the region picker, remembers the choice in `%TEMP%\solo_steam.cfg`, starts `gujianol.exe` with `ShellExecuteExW` and waits for it to exit. The picker runs in a hosted web-browser control, which could fetch remote content by itself. | **Inferred** | S, TS, TP |
| 5.4 | `gujianol.exe` contains none of `solo_steam`, `.cfg`, `serverlist`, `server_list`, `region=` or `gfclient`, in ASCII or UTF-16. | **Observed** | S |
| 5.5 | So the game probably doesn't read `solo_steam.cfg`: the file is the launcher's memory of the region. This rests on absent strings, and compressed scripts can split a string beyond the reach of a plain search (3.12). | **Inferred** | S |
| 5.6 | The command line the launcher passes to the game. The triage report says disassembly showed it, but no output was kept, so the claim is dropped. | **Unknown** | T |
| 5.7 | `gujianol.exe` imports six functions from `gameforge_api64.dll`: `GameforgeApi_LoadSdk`, `GameforgeApi_GetPlatform`, `GameforgeApi_IsRunning`, `GameforgeApi_Init`, `GameforgeApi_GetContext` and `GameforgeApi_Shutdown`. FUN_1401A3580 calls FUN_1401A34A0 with the constant 1, and FUN_1401A34A0 calls `GameforgeApi_LoadSdk` with it, then `IsRunning`, `Init` and `GetContext(0xC)`. | **Observed** | V02, G |
| 5.8 | Which SDK the value 1 selects. | **Unknown** | G |
| 5.9 | `gameforge_api64.dll` (2.1.3.123, unsigned, PDB under `tnt\gameforge-client-library`) exports 38 functions, among them `GameforgeApi_IUser_GetAuthSessionTicket`, `GameforgeApi_IUser_GetAuthCode`, `GameforgeApi_IUser_GetAccountId`, `GameforgeApi_IUser_GetNumericAccountId`, `GameforgeApi_IUser_CancelAuthTicket` and `GameforgeApi_IApps_GetCurrentGameRegion`. Its only regular imports are `KERNEL32`, `ADVAPI32`, `CRYPT32` and `SHLWAPI`. It delay-imports `steam_api64.dll` (12 functions, including `SteamAPI_Init` and `SteamInternal_CreateInterface`) and `EOSSDK-Win64-Shipping.dll` (20, including `EOS_Auth_Login` and `EOS_Auth_CopyUserAuthToken`). The EOS DLL is not shipped; `steam_api.dll` and `steam_api64.dll` are. | **Observed** | V09, V10, V11, TS |
| 5.10 | The library's strings include the flags `--gf-sdk`, `--gf-appid`, `--steam-appid`, `--gf-region`, `--mocked-appid`, `--mocked-region`, `--mailru-appid` and `--gf-sgs-enabled`; the SDK names `steam`, `mocked`, `mailru` and `EpicGames`; the Gameforge client names `GC_PIPE_NAME`, `GC_USER_NAME` and `GC_GCLAY64_PATHNAME`, and "GameCenter is not running"; the config file `gfclient.ini`, which the install doesn't have; Steam interface names such as `SteamUser019`; the keys `auth_ticket`, `account_display_name` and `external_account_id`; and an Iovation module. | **Observed** | V09, V13, TS |
| 5.11 | The library reads these flags from the process command line. On the Steam path it gets a session ticket through the `SteamUser019` interface, which needs a running Steam client. On the Gameforge path it talks to the Gameforge client over a named pipe. | **Inferred** | V09, TS |
| 5.12 | A `mocked` SDK with its own app ID and region flags exists. What it does, and whether it is a usable input that the client already accepts (design spec §5.3, option 1). | **Unknown** | V09 |
| 5.13 | Ticket bindings exist by name: `steam_get_actor_ticket`, `gameforge_get_ticket`, `gameforge_get_account*` and `wegame_*`. | **Observed** | V09 |
| 5.14 | The login scripts fetch a platform ticket and send it to the server. | **Inferred** | V09 |
| 5.15 | Which message carries the ticket: `CTS_PROTOCOL_CLIENT_ENTER`, a `GATE_SCRIPT` message, or the payload of the first native message (4.25). | **Unknown** | |
| 5.16 | The message "FAILED TO FETCH … SERVER LIST!" sits in a compressed script in `gujianol.exe`, at file 0x15CE7C7. | **Observed** | V09 |
| 5.17 | The game itself fetches the server list. | **Inferred** | V09, T |
| 5.18 | Per the triage report, the owner's log shows the full message: "FAILED TO FETCH GAMEFORGE SERVER LIST!" (§7). | **Observed (local)** | T |
| 5.19 | Where the server-list URL comes from. The candidates are the launcher's `serverlist_url` strings, the Gameforge library, and a URL inside a compressed script. | **Unknown** | |
| 5.20 | `bin64/loader.ini` has a `[CONFIG]` section with `WANGYUAN_DNS_RECOMMEND=120.133.237.40` and `WANGYUAN_DOMAIN=authentication.wangyuan.com/`. No binary in `bin64/` names these keys in plain text. | **Observed** | V09, V13, S |
| 5.21 | `loader.ini` configures the Chinese publisher's (Wangyuan) authentication and is probably unused by the Gameforge build. This too rests on absent strings, which compressed scripts can hide (3.12). | **Inferred** | S, V13 |
| 5.22 | `bin64/sentry.ini` holds only local paths: `Sentry_Base = ./sentry`, `Sentry_Attachment = ./gujianol.log`. The crash reporter `AurCrashRpt64.dll` has the configuration keys `HTTPServer`, `SMTPSetver`, `SMTPLogin` and `SMTPPassword`. Their values live in the obfuscated `bin64/AurReport*.ini` files, which were not decoded. The triage saw credentials there; they are left out here and must never be used. | **Observed** | S, V13, T |
| 5.23 | A crash may make the client try to upload a report over HTTP or SMTP. | **Inferred** | V13 |
| 5.24 | The normal start depends on two services that are now dead: the Gameforge server list (seen failing, §7) and whatever validated the ticket. The exe reached its login scene without them. | **Inferred** | T, V09 |
| 5.25 | Whether the client needs a running Steam client. | **Unknown** | V09 |

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
| 6.22 | A region of uncompressed script source in `.rdata` (around file 0x1191000) holds the runtime's built-in helpers, such as the remote debugger (4.32). It is written in a Lua dialect with type annotations. | **Observed** | TS |
| 6.23 | Table paths occur under `tables/item/`, `tables/player/`, `tables/quest/`, `tables/present/` and `tables/cutscene/`, and 35 distinct `.tab` names in plain text. | **Observed** | S |
| 6.24 | The tables themselves are in the archives. | **Inferred** | S |
| 6.25 | With TinyCC on board (`tinycc_add_cobject`, `__tccmacros__`, the `--tcc-obj` flag), the runtime can compile C code at run time. | **Inferred** | V10, V09, S |

## 7. The owner's launch (2026-10-11)

The owner launched the game once through Steam on the Fedora box, under Proton GE-Proton11-7, right after the download finished at 19:32:45 AEDT. Proton set up its prefix from 19:32:45 to 19:32:55 (7.4), and the game's last writes came at 19:33:36. No kept file records when the game process itself started; its first write in the install came at 19:33:06 (`bin64/Log_gujianol.htm`, 7.1). The logs are still in the owner's Steam install and the prefix in Steam's `compatdata`; this section uses the triage's listings and report.

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 7.1 | Writes in the install: `bin64/Log_gujianol.htm` (19:33:06), `settings/GameSetting.dat` (19:33:10), `settings/PanelPositionSetings.dat` and `bin64/gujianol.log` (19:33:36), and the empty directories `settings/`, `ExportFace/` and `ExportPrefab/`. Nothing else in the install changed. | **Observed (local)** | PB, TI |
| 7.2 | Per the triage report, the client's log shows: engine start on an AMD GPU (RADV) under Proton, table cache loaded, "Load Login Scene", then the error "FAILED TO FETCH GAMEFORGE SERVER LIST!", "SETUP LOGIN STAGE DONE", a net error 5 while waiting, with the net state `NET_ST_CLOSED(5)`, and a clean shutdown. | **Observed (local)** | T |
| 7.3 | The client reached its login scene with the servers gone and stopped at the server-list fetch. `NET_ST_CLOSED` = 5 matches the position of `KS_NET_CLIENT_ST_CLOSED` (4.11). | **Inferred** | T, V04 |
| 7.4 | The prefix's timeline. Proton's prefix set-up, 19:32:45–19:32:55: the prefix skeleton and a registry backup (19:32:45), an NVIDIA DLL (19:32:51), the VC++ 2013 redistributable (19:32:52), Wine's DLLs and drivers and Steam's client files (19:32:54), and the `wine_gecko` profile (19:32:55). Then, while the game ran: DXVK's state cache (19:33:01–19:33:13), the NVIDIA GfeSDK log `gujianol_856.log` (19:33:07; per the triage, a failed initialisation on the AMD GPU), the registry flush (19:33:24) and `%TEMP%\solo_steam.cfg` (18 bytes at 19:33:36; `region=na` per the triage). | **Observed (local)** | TP, T |
| 7.5 | `solo_steam.cfg` was written by the launcher, the only binary that names it, as the game exited. 18 bytes fit `region=na` in UTF-16. | **Inferred** | TP, V13 |
| 7.6 | Per the triage, the run added only two registry keys: the game's install directory under `HKCU\Software\GuJian\GuJianOL` and Steam's own app key. No service or driver was registered. The registry diff was not kept. | **Observed (local)** | T |
| 7.7 | No AGT driver file appeared: no `.sys` file has an AGT, ProcessProtect or Aurogon name, and all 24 are Wine's own (3.13). | **Observed (local)** | V14 |
| 7.8 | The personal data the triage saw is left out: the Steam manifest's owner fields and the crash reporter's credentials. The triage found no account name, e-mail address or token written by this launch. | **Observed (local)** | T |

## Open for runtime (Task 7)

What the isolated run should watch for. Everything here comes from the static reading above, and none of it has been confirmed at run time.

Names to watch (DNS lookups and connects):

| Source | Names |
|---|---|
| `gujianol.exe` (4.36) | `upgrade-api.wang…` (probably `upgrade-api.wangyuan.com`, path `/gate?gid=105`), `gjol.wangyuan.com`, `gjolintl.wangyuan.com`, `pay.wangyuan.com`, `service.wangyuan.com`, `members.cibmall.net`, `me.cibmall.net`, `gj.cibmall.net`, `cibmall.net`, `wenjuan.feishu.cn`, `forum.solo.gameforge.com`, `solo.support.gameforge.com`, `www.youtube.com`, `code.jquery.com`, `gburl.cn`, `burl.cn`, `store.s…` |
| `bin64/loader.ini` (5.20) | `authentication.wangyuan.com`, and 120.133.237.40 as a recommended DNS server |
| `SOLO_Launcher.exe` (5.2) | `gameforge.com`, only if the launcher runs |
| SDKs and helpers, names not in the strings | the Gameforge server list (5.16, 5.17 and 5.19; the first name to look for), Steam (`steam_api64.dll`), the Gameforge client's pipe, crash upload (Sentry, crashpad, `AurCrashSender64.exe`), CEF pages (`browserhost.exe`), Vivox, NVIDIA GfeSDK |
| Authenticode blocks | the DigiCert, Entrust, WoSign and Microsoft CRL and OCSP hosts (thawte and Symantec for the launcher), only if something checks revocation |

Addresses and ports:

| Address | Context | Note |
|---|---|---|
| `121.5.97.203:800` | patch download (4.34) | port 800 can't be bound under `--no-root`; read the attempt from Wine's log |
| `140.210.69.34:7804` | captcha (4.34) | |
| 120.133.237.40 | `loader.ini` DNS recommendation (5.20) | |
| 127.0.0.1, UDP 3756–3766 | remote-debugger announcement (4.32) | expected only with a debug proxy |
| the game server | the address list scripts hand to `net_client_create` (4.22) | port Unknown; it should come from the server list |

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
2. Use of the Service Control Manager beyond the read-only `CryptSvc` status query that static reading expects (`OpenSCManagerW`, then `OpenServiceW` on `CryptSvc`, `QueryServiceStatus` and `CloseServiceHandle`; 3.9–3.10): any `CreateServiceW`, `StartServiceW`, `NtLoadDriver` or `ZwLoadDriver`, or a new key under `HKLM\System\CurrentControlSet\Services`. Read-only lookups of `SYSTEM\ControlSet001\Services\nvlddmkm` and `SOFTWARE\NVIDIA Corporation\Global` from the same module are expected: those are NVIDIA's keys, not AGT's.
3. Any open of `\\.\ProcessProtect`, `\Device\ProcessProtect` or `\DosDevices\Global\ProcessProtect`, and any `DeviceIoControl`.
4. `FindResource` or `LoadResource` on type `PE`, ID 144. Neither is imported, so such a call would have to come through `GetProcAddress`.
5. `GetProcAddress` lookups of driver or service functions from scripts or TinyCC code.

Log lines to look for: "Load Login Scene", "FAILED TO FETCH GAMEFORGE SERVER LIST!", "SETUP LOGIN STAGE DONE" and `NET_ST_CLOSED(5)`.

## 8. Runtime observation (isolated, stub only)

Nine runs of the pinned client, runs 1–9, on 2026-10-11 and 2026-10-12, by an agent that has not read any source rejected in [`sources.md`](sources.md). Each run happened inside a frozen copy of `scripts/spike/isolated-run.sh` with `--no-root --x11 99`: loopback only, names from a generated hosts file, and a dedicated rootful Xwayland `:99` that the coordinating agent started for the runs, with the owner's standing permission. For runs 1–6 the copy was made from 60d8b36, the head of #10, whose `scripts/spike/` equals f8d3748's on `dev`, and it was checked byte-identical to f8d3748's before run 5. Before run 7 it was refreshed to fbfd4d5, whose hash runs 7–9 record. From run 7 the sandbox also had `--cpu-topology` (#14), which binds the host's `/sys/devices/system/cpu` read-only so that Wine has processor information; nothing else of `/sys` is inside. Steam was not running: the wrapper refuses to start otherwise, and it was checked before runs 5 and 7.

Before these runs, the client was started twice in the same kind of sandbox through `re/spike/play.sh`, an uninstrumented runner (loopback only, no stub, no watchdog). The first start was the owner's own try (22:48 AEDT on 2026-10-11). The second was the coordinating agent's environment check with Proton's default log channels (22:49), to see why the client returned at once. Both ended like run 1, with Proton exit 255; the second's Wine log shows the client gone about 35 ms after it loaded, and the first kept no Wine log. The kept `play.sh` sets `PROTON_LOG=0` and no `SteamGameId`, so it reproduces only the first start: the second ran with Proton's log on (`SteamGameId` 1418100, Proton's default channels), which the kept file can't give. These were environment checks, not runs. They weren't against the stub, they don't count against the run cap, and the findings below cover runs 1–9 only (key P0).

The client was the pinned build (`bin64/gujianol.exe` as above, `bin64/gameforge_api64.dll` SHA-256 `1250487a…908f`), started from a throwaway reflink copy with GE-Proton 11-7's own `proton run`. It ran non-root, with no Steam client files and Mesa's lavapipe for Vulkan. umu's runtime container needs `/sys`, so it wasn't used. `fenji-stub` listened in the same sandbox on `127.0.0.1:7804`, a placeholder port, because no run showed a real one. The hosts file stayed empty, because no run resolved a name.

Wine debug channels:
- runs 1–6: Proton's default set plus `+winsock,+module,+process,+loaddll,+profile,+reg,+environ,warn+all`;
- runs 7–9: a lean set, `+timestamp,+pid,+tid,+threadname,+seh,+debugstr,+loaddll,+winsock,+module,+process,+service,+profile,warn+file`.

From run 5 a driver watchdog on the host polled each run every 0.5 s, ready to freeze and kill the sandbox at a sign of driver activity. It watched for:
- a new or changed `.sys` file in the run's workspace or the sandbox's temporary mounts;
- a `winedevice.exe` started after the client (Wine starts its own two before the client);
- driver signs in Wine's log: a native `.sys` load, a lookup of resource type `PE`, a driver load or service creation outside Wine's own, and the driver's strings.

The client started only after the watchdog had armed itself. The watchdog is best effort, though. Polling can miss a short-lived process: in run 5 the client lived 103 ms, and the watchdog never saw it as a process. Its live log scan can also fall behind a fast-growing log: in run 6 it had reached 4.93 GB of the 12.15 GB log when the client ended, and it scanned the rest after the run. So the AGT answer rests on the complete checks after each run: the `.sys` and service-key comparisons, the watchdog's full scan of each Wine log, and the reading of the evidence below.

No client file was modified. The client created `settings/`, `ExportFace/` and `ExportPrefab/` in its own copy; after each run these were moved into that run's evidence, so the next run started from the pinned layout.

Static reading for this section used read-only Ghidra queries (addresses in the pinned exe) and `objdump` on `gameforge_api64.dll`. No REA was used. In this section, **Observed** means seen in a run's evidence. The runner and the watchdog (`re/spike/run.sh`, `re/spike/watchdog.py`) are git-ignored local files. The kept `run.sh` is run 9's revision and the kept `watchdog.py` is runs 8–9's. Of the earlier revisions only `re/spike/run-r1-r4.sh` was kept, and it is runs 2–4's runner: it was last changed at 23:05, between runs 1 and 2, so run 1's revision is lost. Runs 1–4 had no watchdog. From run 5 each run's `inputs.txt` records the watchdog's hash, and from run 7 the wrapper's. So the runs can only be repeated approximately for now; a tracked runner is a later milestone's job.

**Gate #11** (decided by the owner on 2026-10-11): option (c), `--gf-sdk=mocked`, the test backend built into `gameforge_api64.dll`. It is allowed for sandboxed dev runs on this copy only, never for players; the player path is a separate M3 decision. Every other gate still applies. It let runs 5–9 pass platform init without Steam or the Gameforge client.

| Run | Change | Login scene? | Names resolved | Ports tried | AGT driver | Stub sessions |
|---|---|---|---|---|---|---|
| 1 | None: no arguments, empty hosts file | No. Exits with -1 after about 50 ms in platform init; no window | None | None | No sign | None |
| 2 | `--gf-sdk steam --steam-appid 1418100`, values after a space | No. Same as run 1: values after a space aren't read | None | None | No sign | None |
| 3 | `--gf-sdk=steam --steam-appid=1418100` | No. Steam SDK selected; `SteamAPI_Init` can't load `steamclient64.dll`; -1 after about 50 ms | None | None | No sign | None |
| 4 | Run 1 plus `GFCLIENT_CONFIGURATION_FILE` pointing at our own `gfclient.ini`, which holds only `logLevel=all` | No. The library logs `Error: 4 gameforge_client_api64_v2.dll`; -1 after about 50 ms | None | None | No sign | None |
| 5 | `--gf-sdk=mocked` (gate #11) | No. Platform init passes, but no archive is mounted and the first table load fails; exits with 1 after about 100 ms | None | None | No sign; watchdog quiet | None |
| 6 | Run 5 plus `--fs=data:_index:_index` | No. The archives mount; 1 s in, thread-affinity set-up loops forever because Wine has no CPU topology without `/sys`; stopped by hand after about 150 s; no window | None | None | No sign; watchdog quiet | None |
| 7 | Run 6 plus `--cpu-topology` (#14); lean channels | Not reached: a watchdog false positive stopped the run 0.118 s after the watchdog saw the client (8.36) | None | None | No sign (the trigger was Wine's own driver) | None |
| 8 | Run 7 again, with the corrected watchdog | **Yes.** The login scene renders, under the error "Interner Serverfehler: None", after "FAILED TO FETCH GAMEFORGE SERVER LIST!"; full 300 s | None | None | No sign; watchdog quiet | None |
| 9 | Run 8 plus `--mocked-region=na` | **Yes**, the same as run 8 | None | None | No sign; watchdog quiet | None |

In short:
- With the mocked backend, the archive argument and a CPU topology for Wine, the unmodified client reaches its login scene under lavapipe (8.15–8.18).
- It resolves no name through Winsock, opens no connection and sends nothing; the server-list fetch fails without any network attempt (8.20–8.25, 8.42).
- None of runs 1–9 showed any sign that the AGT driver is extracted or loaded (8.26–8.31).
- The stub received nothing, not even a zero-byte connection (8.24).

Start-up and platform init:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 8.1 | Runs 1–4 ended 48–54 ms after the client started, with exit code -1 (Proton 255), inside `gameforge_api64.dll`'s platform init. There was no window and no `gujianol.log`, and the client made no Winsock call. | **Observed** | R1–R4 |
| 8.2 | Before it picks an SDK, the library looks for a Gameforge client configuration, in this order: env `GFCLIENT_CONFIGURATION_FILE`; `HKLM\SOFTWARE\(WOW6432Node\)Gameforge4d`; env `_TNT_CLIENT_APPLICATION_ID`; the Gameforge client's Inno Setup key `…\Uninstall\{d3b2a0c1-f0d0-4888-ae0b-1c5e1febdafb}_is1`; and `\gfclient.ini` at the root of drive Z:. All were absent; run 4 set the first, and the library then skipped the rest. Each missing key raises a `std::system_error` that the library catches. Runs 7–9 don't trace registry reads, so there the two missing keys show only as those two caught exceptions. Then the library asks for its parent's image name, which is Proton's `steam.exe`. | **Observed** | R1–R9 |
| 8.3 | Run 2's space-separated values (`--gf-sdk steam`) weren't read: it behaved exactly like run 1. Run 3's `--gf-sdk=steam` selected Steam. | **Observed** | R2, R3 |
| 8.4 | The value 1 that the exe passes to `GameforgeApi_LoadSdk` (5.7, 5.8) means "the SDK named on the command line". In `gameforge_api64.dll`, the export (0x1800028F0) dispatches to 0x180008040, which switches on the SDK type. For type 1 it asks the command-line module for the SDK; that module's function 0x180011890 reads `--gf-sdk`. The parser splits each argument at `=` (around 0x180012358), so the library takes flag values only in `--name=value` form. `--gf-sdk=` takes `gf`, `steam`, `mocked` or `mailru`, with `gf` for a missing or unknown value; env `AUTH_TYPE=exchangecode` selects Epic. The app-id flag goes with the SDK (5.10). Only `gf` and `mocked` read a region flag. | **Inferred** | RL |
| 8.5 | On the default `gf` path (runs 1, 2 and 4) the library looks for `gameforge_client_api64_v2.dll`, finds none, and ends the process with a `GfApiException`. In run 4 it wrote its own log, `Error: 4 gameforge_client_api64_v2.dll`, and read only the `logLevel` and `password` keys of `[GameforgeClient]`; `password` was empty, and we didn't set it. | **Observed** | R1, R2, R4 |
| 8.6 | The `gf` path needs an in-process DLL from the Gameforge client's install. The library finds it through `_TNT_CLIENT_APPLICATION_ID` (default `{d3b2a0c1-…}`) and that Uninstall key's `InstallLocation`, read in 0x1800192C0. The loader, 0x1800975B0, then calls `SetDllDirectoryW` and `LoadLibraryW("gameforge_client_api64_v2.dll")`. The named pipe in 5.11, if there is one, would sit behind that DLL. `GC_PIPE_NAME`, `GC_USER_NAME` and `GC_GCLAY64_PATHNAME` are each used by one function, and all three functions are virtual methods of the library's `MailRuApiModule` class (by its RTTI). This revises 5.10's "Gameforge client names" and the pipe line under "Open for runtime". | **Inferred** | RL |
| 8.7 | On the `steam` path (run 3) the library loaded the client's own `steam_api64.dll`. `SteamAPI_Init` then failed to load `C:\Program Files (x86)\Steam\steamclient64.dll`, with no `steam://` or ShellExecute attempt. The Steam path can't work in this sandbox by design. | **Observed** | R3 |
| 8.8 | The `mocked` path (runs 5–9) passes platform init without Steam, without the Gameforge client and without `--mocked-appid`. Adding `--mocked-region=na` changed nothing visible. With this dev-only backend no Steam client is needed; for players, 5.25 stays open (gate #11). | **Observed** | R5–R9 |

Archives, and the sandbox's CPU topology:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 8.9 | In run 5 the client wrote its own `gujianol.log` for the first time: `load data/caches/protocol_ver.new_cache failed!`, then `load tab file(data/core/xobject/actor_class_define.tab) failed!` and a script error in `__builtins__/__script_system__.lua` (line 118). It then exited with 1, about 100 ms in. Nothing tried to open `data/_index` or a `dataNNN` part. | **Observed** | R5 |
| 8.10 | The root file system (0x14068FB60) mounts the archives only when the command line has `--fs=` (string at 0x142DA5B20). Its value is split at `:` into archive directory, data index and asset index. The directory is relative to the parent of the exe's directory: 0x1400B6580 returns the exe's directory, with the drive letter lowercased. A failed mount exits with 1, and without `--fs=` only `<root>/asset` is mounted, as a plain directory. The editor's `--fs=../data:_index:_index` (6.10) runs from `tools/GujianOL_Editor/` and so reaches the same `data/`. For `bin64/gujianol.exe` the matching value is `--fs=data:_index:_index`. | **Inferred** | RL |
| 8.11 | With `--fs=data:_index:_index` (run 6) the archives mounted. The client opened `data/` and probed for parts beyond `data099`, which don't exist. | **Observed** | R6 |
| 8.12 | One second into run 6, Wine logged `init_logical_proc_info Failed to get logical processor information, status 0xc0000002` in the client; wineboot logs the same in runs 1–6, which had no `/sys`. The main thread then called `GetMaximumProcessorGroupCount` once and `GetMaximumProcessorCount` 178.8 million times, its 16-bit group number wrapping 2,728 times, until the run was stopped about 150 s in. Its Wine log was 12 GB. | **Observed** | R6 |
| 8.13 | The loop is in 0x140E63640, which pins a thread to processor N. It finds N's group by subtracting each group's `GetMaximumProcessorCount`, with a 16-bit group counter and no other bound. It can't end while Wine reports 0 processors per group, which it does without a CPU topology. The "Kernel_Threads" set-up at 0x140E68FC0 resolves these APIs at run time. | **Inferred** | RL |
| 8.14 | In non-client checks (Wine's own `cmd.exe`), a read-only bind of `/sys/devices/system/cpu` was enough for Wine to build its topology, and Proton's `WINE_CPU_TOPOLOGY` override did not help without it. With `--cpu-topology`, no run logged the topology failure, and runs 8 and 9 got past the thread-affinity set-up. Run 7 ended 0.13 s in, before reaching it. | **Observed** | RA, R7–R9 |

Login scene:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 8.15 | Runs 8 and 9 started the engine. Their `gujianol.log` shows the Vision Engine's DirectX 11 renderer on DXVK over lavapipe (adapter "llvmpipe (LLVM 22.1.8, 256 bits)") and "Engine initialized". Along the way it logged "NVIDIA Aftermath DX11 Initialize Failed", "Nvidia NGX: GPU is not NVIDIA GPU, NGX skipped init." and "Warning: Bink Direct Sound Create Failed!", and set up input. | **Observed** | R8, R9 |
| 8.16 | The client's log then shows the same sequence as the owner's launch (7.2):<br>"Load tables from cache, result: true"<br>"++++ Load Login Scene"<br>"ERROR FAILED TO FETCH GAMEFORGE SERVER LIST!"<br>"[GAME_SYSTEM] SETUP DONE"<br>"SETUP LOGIN STAGE DONE"<br>"net error during waiting: 5"<br>"[login...] LoginTimer, Net State: NET_ST_CLOSED(5), Close Reason: None" | **Observed** | R8, R9 |
| 8.17 | The screenshots show the client's own splash logo at 5–10 s. From 30 s there is a modal dialog, "Fehler", "Interner Serverfehler: None", with the button "Spiel verlassen". From about 120 s the full login scene is visible behind it: the background art, the logo, a menu (Lizenzen, Credits, Kooperationspartner, Systemeinstellungen, Spiel verlassen), the publishers' logos and "Client-Version: 2.1.19.42851". The interface is in German. | **Observed** | R8, R9 |
| 8.18 | The client reaches its login scene in the sandbox, and stops where the owner's launch stopped (7.3): no server list, so the net client closes and the login screen shows an internal server error. Bink's DirectSound failure (8.15) fits the sandbox having no audio daemon. | **Inferred** | R8, R9 |
| 8.19 | Why the interface is German: the mocked backend's language, or the client's default for a language it doesn't know. | **Unknown** | R8, R9 |

Network:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 8.20 | In none of runs 1–9 did the client resolve a name through Winsock, open a connection or send anything. No client thread called `getaddrinfo`, `gethostbyname`, `connect` or `sendto`; 8.42 covers lookups outside Winsock. Runs 1–4 made no Winsock call at all; runs 5–9 called `WSAStartup` one to four times. Nothing below port 1024 was tried. | **Observed** | R1–R9 |
| 8.21 | The only sockets were in runs 8 and 9: one IPv6 datagram socket per run, created and closed at once, with no bind, connect or send. It was created about 5 s after the client started, in the same second as the client's "Load Login Scene" and server-list failure lines and its probes for `servername` and `D:\wy_debug.json` (8.32). The client also made ioctl 0x121000 on another handle, in pairs. Run 8 had 174 of them, starting about 14 s in, every 2.1–5.5 s (mean 3.3 s); run 9 had 178, starting about 9 s in, every 2.4–4.1 s. | **Observed** | R8, R9 |
| 8.22 | The IPv6 socket is a capability probe of the kind libcurl makes when it starts. 0x121000 is Wine's network-interface enumeration (device 0x12, function 0x400); the client had loaded `IPHLPAPI.DLL` and `nsi.dll`, and was polling its network adapters. | **Inferred** | R8, R9 |
| 8.23 | The only name lookup in the logs of runs 1–9 is Wine's own: wineboot resolving the sandbox's host name `isolated-run` from the generated hosts file. None of the names under "Open for runtime" was looked up. | **Observed** | R1–R9 |
| 8.24 | The stub received no connection in runs 1–9, not even one with 0 bytes; it records every connection it accepts. | **Observed** | SL, R1–R9 |
| 8.25 | The server-list fetch fails before any network access: no lookup and no bind, connect or send. The only socket, the IPv6 probe (8.21–8.22), was created and closed at that moment. Started directly, the client lacks the server-list source; this isn't a dead service answering. The client never connected to a game server (4.22), so the handshake reading (4.23, the server speaks first) wasn't exercised. | **Inferred** | R8, R9 |

AGT driver:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 8.26 | The client loaded no `.sys` file in runs 1–9. The only `.sys` modules loaded were Wine's own drivers, inside Wine's own `winedevice.exe`. In each of runs 1–9 both `winedevice.exe` started before the client, and the client started no process. | **Observed** | R1–R9 |
| 8.27 | The client never looked up the resource type `PE` (3.4): runs 5–9 were checked as they ran, and runs 1–4's logs afterwards. Its lookups were all standard resource types (cursor, icon, menu, string, version) plus Wine's theme resource types. | **Observed** | R1–R9 |
| 8.28 | The prefix's 24 `.sys` files, all Wine's own, never changed, and no new one appeared. From run 5 the watchdog checked the run's whole workspace and the sandbox's temporary mounts too, and found none. None of the files the client created is an executable. The only new service key in runs 1–9 is Proton's own user-mode `discord-bridge`, added in run 1. | **Observed** | R1–R9 |
| 8.29 | From run 7, with service calls traced, the client made no Service Control Manager call at all, not even the read-only `CryptSvc` status query that static reading expects (3.9–3.10). Its only service-channel calls were device-change notifications (`I_ScRegisterDeviceNotification`). The driver's names never appear in any log, and the only failed `GetProcAddress` lookups are two kernel32 file-information functions. | **Observed** | R8, R9 |
| 8.30 | Up to the login scene, no code path extracts or loads the AGT driver, which agrees with 3.11. | **Inferred** | R1–R9 |
| 8.31 | Whether a later code path would, after login or in game, which these runs never reached (3.12). Successful `GetProcAddress` lookups and device ioctls other than network ones aren't logged by these channels. | **Unknown** | |

Files, modules and processes:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 8.32 | Files the client looked for and didn't find: `Z:\gfclient.ini` (every run but run 4, where the library read our own `gfclient.ini` instead, 8.5); `client/servername` and `D:\wy_debug.json`, around login-stage set-up (runs 8 and 9); `bin64/../VERSION`; and `settings/LanguageSetting.dat` and other `settings/*.dat` files before it had created `settings/`. It looked for neither `solo_steam.cfg` nor `steam_appid.txt`. Through the INI API it read only Wine's `system.ini` (the audio codec list) and, in run 8, `desktop.ini` files, never `loader.ini` or `sentry.ini`. | **Observed** | R1–R9 |
| 8.33 | The game doesn't read `solo_steam.cfg`, at least up to the login scene (5.5). Whether it reads `loader.ini` through some other file API stays open, because successful opens aren't logged (5.21). | **Inferred** | R8, R9 |
| 8.34 | Writes, in runs 8 and 9. In the install: `settings/GameSetting.dat` and the empty `ExportFace/` and `ExportPrefab/`, as in the owner's launch (7.1). In the prefix: the NVIDIA GfeSDK log and DXVK's state cache. | **Observed** | R8, R9 |
| 8.35 | In runs 8 and 9, besides its static imports (`DSOUND` and `IPHLPAPI` among them; 2.4, 4.1) and DXVK's `d3d11` and `dxgi`, the client's process loaded Wine's `dinput8`, `xinput1_3`, `mmdevapi`, `winepulse.drv`, `winevulkan`, `nsi`, `WININET` and `dnsapi`. In none of runs 1–9 did it load `libcef.dll` or `psw_tnt64.dll`; it loaded `steam_api64.dll` only in run 3. It started no child process: no `browserhost.exe`, `crashpad_handler.exe` or `AurCrashSender64.exe`. | **Observed** | R1–R9 |

The watchdog:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 8.36 | A watchdog false positive stopped run 7, 0.118 s after the watchdog saw the client. Wine's driver host had failed to load Wine's own Bluetooth driver (`winebth`, c0000135). Wine's error message about it was split in two by a nested trace, so the watchdog's driver-load rule matched a line without a readable service name. That line was logged at 39666.696, before the client's creation at 39666.858, and the same `winebth` error appears in the logs of runs 1–9. The stop was ruled a false positive. From run 8 the watchdog reads the name from the message's continuation, and logs driver-load lines from before the client's creation as notes; everything else still stops the run. | **Observed** | R7, RL |

Open after these runs:

| # | Finding | Tag | Evidence |
|---|---|---|---|
| 8.37 | Started directly, what would give the client its server list or a server address. The candidates are what the launcher passes on (5.6), the `servername` file at the install root, the flags `--server-name`, `--server_locale` and `--net`, and the server-list URL (5.19). | **Unknown** | R8 |
| 8.38 | What the client sends first, and whether it really waits for the server's 64 bytes (4.23, 4.25). A listen-only stub would see a connection with 0 bytes. | **Unknown** | |
| 8.39 | What `data/caches/protocol_ver.new_cache` is. It isn't a loose file in either install, though it may live in the archives. Run 5 logged a failed load of it, and that wasn't what stopped the client. | **Unknown** | R5 |
| 8.40 | Why the net layer trains an Oodle Network dictionary at start-up from almost no data. Run 6's Oodle debug output said "test_packet_total_size = 1" and "not enough packets to fill dictionary!" (4.19–4.21). | **Unknown** | R6 |
| 8.41 | Which other sandbox gaps a later stage would hit, for example audio, CEF pages or the GPU. | **Unknown** | |
| 8.42 | Whether the client looks names up through `DnsQuery`. Its process loaded `dnsapi` (8.35), and Wine resolves `DnsQuery` calls on the Unix side, outside Winsock, so `+winsock` doesn't show them. With no DNS server in the sandbox such a lookup would fail, but it wouldn't appear in these logs. | **Unknown** | R8, R9 |

## Conclusions

M0's three questions (design spec §12), answered from §1–§8 by an agent that has not read any source rejected in [`sources.md`](sources.md). The Evidence column points at this document's rows, which name the evidence keys. Confidence is high where the runs showed it and the static reading agrees, and low where only static reading, or nothing, speaks to it.

| Question | Answer | Confidence | Evidence |
|---|---|---|---|
| Does `gujianol.exe` reach its login scene without `SOLO_Launcher`, in the sandbox? | **Yes** (**Observed**, runs 8 and 9). Started directly with `--gf-sdk=mocked` (dev-only, gate #11) and `--fs=data:_index:_index`, in a sandbox with `--cpu-topology`, the unmodified client renders its login scene. It stops where the owner's launch through Steam stopped, at "FAILED TO FETCH GAMEFORGE SERVER LIST!", and shows a dialog "Interner Serverfehler: None". It needed all three. Without a usable platform SDK it exits about 50 ms in, inside `gameforge_api64.dll` (runs 1–4); without `--fs=` its first table load fails (run 5); without a CPU topology for Wine it loops in its thread-affinity set-up until stopped (run 6).<br>**Open for M3:** the players' platform SDK. `mocked` is for dev runs only; the Steam SDK needs Steam's own client, and the `gf` SDK the Gameforge Client's DLL. | High | 7.2–7.3, 8.1–8.18 |
| Is the AGT kernel driver ever extracted or loaded? | **No, up to the login scene.**<br>**Observed** (runs 1–9): no sign of extraction or loading. The client loaded no `.sys` file and never looked up the driver's resource type `PE`. No `.sys` file appeared or changed, and no service key appeared besides Proton's own. Where service calls were traced (from run 7), the client made no Service Control Manager call at all, and the driver's names appear in no log. Every run was checked afterwards, and a watchdog watched runs 5–9.<br>**Inferred:** so no code path up to the login scene extracts or loads it, in line with the static reading that nothing in the exe does.<br>**Unknown:** code paths after login and in game, which no run reached, and calls these logs don't show, such as successful `GetProcAddress` lookups. | High, up to the login scene | 3.4–3.13, 7.7, 8.26–8.31 |
| What does the client send first, and where does it get its server address? | **Unknown at run time.** In runs 1–9 the client resolved no name through Winsock, connected nowhere and sent nothing, and the stub received nothing, not even an empty connection (**Observed**). Started directly, it lacks the source of its server list: the fetch fails without a name lookup through Winsock or any connection (**Inferred**; lookups outside Winsock stay open).<br>**Static reading** (**Inferred**, untested): the client connects to IPv4 addresses that its scripts hand to `net_client_create`, sends nothing until the server's 64-byte hello, then queues a first message with 16 bytes from the shared dictionary, 6 bytes from `SendARP` and a payload, perhaps the login ticket.<br>**Unknown:** where the address or the server list comes from. The candidates are what the launcher passed on, the `servername` file at the install root (the client looks for it), the flags `--server-name`, `--server_locale` and `--net`, and a server-list URL. | Low | 4.22–4.28, 5.6, 5.15–5.19, 8.20–8.25, 8.32, 8.37, 8.38, 8.42 |

## Recommendation for M1–M2

Proceed: nothing in M0 stops the project, since the client is readable (§2), reaches its login scene in the sandbox unmodified, with flags it already accepts (its SDK flag, `--gf-sdk=mocked`, is for dev runs only, #11), and showed no sign of the AGT driver up to there (§8). Of the design spec's §5.3 options the evidence favours option 1, inputs the client already accepts: flags already choose its platform SDK and mount its archives (8.4, 8.10), it looks for a `servername` file at its install root and has `--server-name`, `--server_locale` and `--net` flags (8.32, 8.37), and it looked up no host name through Winsock, so option 3, a hostname redirect, has nothing to redirect so far (8.20–8.23, 8.42). M1's first target is the login-stage scripts: decompress and disassemble enough of the embedded Lua (6.12–6.19) to find what gives the client its server list or address (8.37), and, if the same scripts show it, why its interface is German (8.19). M2's first target, once that input makes the client connect to the stub, is to confirm that it waits for a 64-byte server hello, answer it and capture its first message (4.23–4.28, 8.38); that also shows how soon our server needs an Oodle Network encoder (4.5), and running Oodle's own code on our server is a decision gate. M1–M2 can't rely on REA Evidence IDs yet, since M0's one attempt produced none and REA hasn't been retried (Method), so they should cite pinned-exe addresses, as M0 did, and add Evidence IDs wherever a targeted REA session works.

## Decision gates

Every issue labelled `decision-gate`, open or closed, on 2026-10-12:

| Issue | Gate | Status | Decision |
|---|---|---|---|
| #11 | Getting the client past platform-SDK init in the sandbox | Closed; decided by the owner on 2026-10-11 | Option (c), `--gf-sdk=mocked`: sandboxed dev runs on the owner's own copy only, never for players. The player path is a separate M3 decision, and every other gate still applies. The decision capped the runs at 8. The controller later raised the cap to 12 in the owner's absence, as a comment on #11 records, and Task 7 used 9. |

No gate is open, so none blocks M1. Task 7 stopped twice more, and the controller ruled on both in the owner's absence, at the owner's request to decide without them (#13):
- after run 6, the opt-in `--cpu-topology` sandbox flag (#13, PR #14), rather than a bind at the call site;
- after run 7, the watchdog's trigger judged a false positive on Wine's own `winebth.sys` (8.36).

Neither stop was raised as a gate issue. The owner may ratify or revisit both rulings and the cap raise. Gates the evidence points to, none raised yet: Oodle for the archives (M1) and for the network codec (M2), if either would use Oodle's own code outside its licence (4.5, 6.6); and a stand-in for the Gameforge Client's DLL (8.6), should M3 consider one as a player path, since our code would then run inside the client process, which #11 judged close to the patching gate.

## Evidence index

Paths are relative to the repo root. `re/` is git-ignored: these files exist only on the owner's machines. Every finding's row above names its keys.

| Key | Path | What it holds | Findings |
|---|---|---|---|
| V00 | `re/triage/verify/00-identity.txt` | exe SHA-256 and size; files and bytes in the pinned copy | 1.1, 1.3 |
| V01 | `re/triage/verify/01-pe-header-sections.txt` | PE header, section table with entropy, debug directory (PDB), version resource, high-entropy windows | 1.3, 1.4, 1.12, 2.1–2.3, 2.7, 2.10, 2.11, 2.14 |
| V02 | `re/triage/verify/02-imports.txt` | import table; which resource, driver, service and debugger APIs are imported | 1.4, 2.4, 2.12, 3.8, 3.11, 3.12, 4.1, 4.4, 4.5, 5.7, 6.6 |
| V03 | `re/triage/verify/03-pdata.txt` | exception-directory counts | 2.5, 2.6 |
| V04 | `re/triage/verify/04-protocol-strings.txt` | `STC_`/`CTS_PROTOCOL_*`, `KS_NET_CLIENT_ST_*` and `prop_*` names | 4.10, 4.12, 4.13, 4.16–4.18, 7.3 |
| V05 | `re/triage/verify/05-lua.txt` | Lua chunk headers, the LZ4 reading, the opcode-name table | 3.12, 6.12–6.19 |
| V06 | `re/triage/verify/06-driver-resource.txt` | the embedded driver: resource entry, header, imports, PDB, strings, signer; references to it outside the resource | 3.4–3.9, 3.11 |
| V07 | `re/triage/verify/07-archives.txt` | archive magic, record tables, first-blob headers, the `_index` header | 6.1–6.6, 6.8, 6.9 |
| V08 | `re/triage/verify/08-network-constants.txt` | constants and import-table call targets at the network-code addresses | 4.19, 4.21, 4.23, 4.24, 4.30 |
| V09 | `re/triage/verify/09-endpoints.txt` | hosts, URLs, IPv4 strings and flags per binary; the Gameforge library's imports and exports; config inputs | 1.18, 1.24, 2.14, 4.20, 4.21, 4.32, 4.34–4.36, 5.2, 5.9–5.14, 5.16, 5.17, 5.20, 5.24, 5.25, 6.25 |
| V10 | `re/triage/verify/10-versions.txt` | version resources in `bin64/`, middleware markers, the Oodle version | 1.5, 1.6, 1.9–1.20, 1.22–1.26, 2.9, 3.12, 4.2, 4.4, 4.8, 5.9, 6.25 |
| V11 | `re/triage/verify/11-signers.txt` | Authenticode signer names and signing times | 1.5, 2.8, 5.9 |
| V12 | `re/triage/verify/12-anticheat-names.txt` | anti-cheat and protector names in file names and binaries | 1.7, 2.9, 3.1–3.3 |
| V13 | `re/triage/verify/13-extra-checks.txt` | launcher strings, Gameforge library details, enums, entropy runs, device paths, crash-reporter keys | 1.7, 1.16, 1.24, 2.10, 3.9, 3.10, 3.15, 3.16, 4.15, 5.2, 5.10, 5.20–5.23, 6.10, 6.11, 7.5 |
| V14 | `re/triage/verify/14-prefix-drivers.txt` | every `.sys` file in the listing of the owner's prefix, with its time | 3.13, 7.7 |
| VG | `re/triage/verify/ghidra-notes.txt` | the first verifier's read-only Ghidra reading of the network code | 3.10, 4.11, 4.22, 4.23, 4.25, 4.29, 4.31 |
| VS | `re/triage/verify/verify_triage.py`, `re/triage/verify/README.txt` | the script that produced V00–V13, and how it was run | Method; produced V00–V13 |
| RC | `re/triage/verify-recheck/00-identity.txt` … `13-extra-checks.txt`, `README.txt` | a second run of the same script, with identical results | Method; reproduces V00–V13 |
| S | `re/triage/verify-recheck/spot-checks.txt`, with `spot-checks.sh` and `spot_checks.py` | independent checks: `sha256sum`, `objdump`, `grep` counts, the small config files, the inventory and the `tools/` file list, Lua headers, the driver image, archive magic | 1.1–1.3, 1.5, 1.7–1.9, 1.20, 1.21, 2.1, 2.2, 2.4, 2.5, 2.7, 2.15, 3.4, 3.8, 3.11, 4.12, 4.16, 5.1, 5.3–5.5, 5.20–5.22, 6.1, 6.12, 6.20, 6.21, 6.23–6.25 |
| G | `re/triage/verify-recheck/ghidra-recheck.txt` | read-only Ghidra checks of VG, the Gameforge SDK load, the process and connection imports, the `IsDebuggerPresent` call sites, the CryptoAPI and Oodle LZ reference sites, and the signature-check module around FUN_140AD4FA0, with its string and global references per function (section 10, added in Task 8) | 2.5, 2.6, 2.12, 2.13, 3.9–3.11, 3.17–3.19, 4.2, 4.5–4.7, 4.11, 4.23, 4.25, 4.26, 4.28, 4.29, 5.7, 5.8 |
| T | `re/triage/triage-report.md` | the 2026-10-11 triage report | 3.14, 5.6, 5.17, 5.18, 5.22, 5.24, 7.2–7.4, 7.6, 7.8 |
| TI | `re/triage/inventory-files.tsv` | listing of the Steam install after the launch, with times | 7.1 |
| TP | `re/triage/prefix-files.tsv` | listing of the Proton prefix after the launch, with times | 3.13, 5.3, 7.4, 7.5 |
| TD | `re/triage/disasm/` | the triage's disassembly of the network code and of libcurl's NTLM path | 4.3, 4.30 |
| TX | `re/triage/xrefs-gujianol-net.txt` | references to network imports and strings (its import-reference addresses are one byte low; G has the exact ones) | 2.12, 3.17, 4.19 |
| TS | `re/triage/strings/` | string dumps: `SOLO_Launcher.*`, `gameforge_api64.*`, `gujianol-builtin-lua-region.txt` | 4.32, 4.33, 5.2, 5.3, 5.9–5.11, 6.22 |
| TR | `re/triage/rea-attempt.txt` | the triage's one REA session: the exe opened, no analysis, no evidence bundle | Method |
| PB | [`pinned-build.md`](pinned-build.md) | the pinned build, and the files the launch created | 7.1 |
| P0 | `re/spike/play.sh`; `re/spike/out/play-protonlog.out`, `play-protonlog2.out`, `steam-1418100.log` | the two pre-task sandboxed starts (2026-10-11, 22:48 and 22:49): the uninstrumented runner as the first start used it (the second ran with Proton's log on), both starts' Proton output and the second's Wine log | §8 set-up |
| R1 | `re/spike/out/r1-bare/` | run 1: inputs, Wine log, exit codes, `.sys` and service snapshots, registry diffs, new files | 8.1–8.2, 8.5, 8.20, 8.23–8.24, 8.26–8.28, 8.30, 8.32, 8.35 |
| R2 | `re/spike/out/r2-steam-sdk/` | run 2: the same set | 8.1–8.3, 8.5, 8.20, 8.23–8.24, 8.26–8.28, 8.30, 8.32, 8.35 |
| R3 | `re/spike/out/r3-steam-sdk-eq/` | run 3: the same set | 8.1–8.3, 8.7, 8.20, 8.23–8.24, 8.26–8.28, 8.30, 8.32, 8.35 |
| R4 | `re/spike/out/r4-gfclient-loglevel/`, with the input `re/spike/inputs/r4-gfclient-loglevel/gfclient.ini` | run 4: the same set, plus the Gameforge library's own log | 8.1–8.2, 8.5, 8.20, 8.23–8.24, 8.26–8.28, 8.30, 8.32, 8.35 |
| R5 | `re/spike/out/r5-mocked/` | run 5: the same set, plus the client's `gujianol.log` and the watchdog's log and baseline | 8.2, 8.8–8.9, 8.20, 8.23–8.24, 8.26–8.28, 8.30, 8.32, 8.35, 8.39 |
| R6 | `re/spike/out/r6-mocked-fs/` | run 6: the same set, with the 12 GB Wine log compressed (`wine.log.zst`, SHA-256 in `wine.log.sha256`) and without its loop lines (`wine-noloop.log`); screenshots; how the run was stopped | 8.2, 8.8, 8.11–8.12, 8.20, 8.23–8.24, 8.26–8.28, 8.30, 8.32, 8.35, 8.40 |
| R7 | `re/spike/out/r7-mocked-fs-cpu/` | run 7: the same set, plus the watchdog's trigger record (`watchdog-trigger.txt`) | 8.2, 8.8, 8.14, 8.20, 8.23–8.24, 8.26–8.28, 8.30, 8.32, 8.35–8.36 |
| R8 | `re/spike/out/r8-mocked-fs-cpu/` | run 8: the same set, plus screenshots, the client's `gujianol.log` and `Log_gujianol.htm`, DXVK's logs and the files the client created (`client-files/`) | 8.2, 8.8, 8.14–8.30, 8.32–8.35, 8.37, 8.42 |
| R9 | `re/spike/out/r9-mocked-region-na/` | run 9: the same as run 8 | 8.2, 8.8, 8.14–8.30, 8.32–8.35, 8.42 |
| RL | `re/spike/out/RUNS.md` | the run log: set-up, every run, the watchdog's design, tests and fixes, and the static addresses read for §8 | 8.4, 8.6, 8.10, 8.13, 8.36 |
| RA | `re/spike/out/aux-cputopo-a-none/`, `aux-cputopo-b-override/`, `aux-cputopo-c-sysbind/` | non-client checks of Wine's CPU topology in the sandbox (Wine's own `cmd.exe`) | 8.14 |
| SL | `re/stub-logs/r1-bare/` … `r9-mocked-region-na/` | `fenji-stub`'s session logs, one directory per run; all empty | 8.24 |
| RW | `re/spike/run.sh`, `re/spike/watchdog.py`, `re/spike/run-r1-r4.sh` | the runner and the driver watchdog as kept: run 9's `run.sh` and runs 8–9's `watchdog.py`, plus `run-r1-r4.sh`, runs 2–4's runner (last changed between runs 1 and 2). Other revisions weren't kept, but from run 5 each run's `inputs.txt` records the watchdog's hash, and from run 7 the wrapper's. All git-ignored and local. | §8 set-up |

REA Evidence IDs: none. REA was tried for this write-up, but its `binary_session` status output was too large to read in this setup, so no REA analysis ran; the triage's one REA session recorded no evidence either (TR).
