# Reverse-engineering workflow

How we analyse the pinned SoLO client without breaking the ground rules (design spec §2, §4).

## Where things live

| Thing | Location | Committed? |
|---|---|---|
| Pinned client copy (exe, DLLs, data archives) | `$FENJI_CLIENT_PATH` (default `~/fenji-client/`), outside the repo | Never |
| Ghidra project, REA sessions, evidence bundles, decompiled output, extracted scripts | `re/` in the repo root (git-ignored) | Never |
| Sandboxed runs: Wine prefix, logs, screenshots, stub logs | `re/spike/`, `re/stub-logs/` (git-ignored) | Never |
| Our own findings in prose | `docs/protocol/`, `docs/re/` | Yes |
| Rust definitions citing evidence | `crates/fenji-proto/`, `crates/fenji-data/`, `crates/fenji-lua/` | Yes |

`scripts/check-forbidden-files.sh` (run in CI and by the pre-commit hook) rejects game binaries, archives, native libraries and Oodle's SDK, Lua bytecode, captures and analysis databases, and the tools' run and decode artefacts wherever they are staged: stub session logs and auth captures, Oodle spool blocks, table caches and dumps.

## Getting the pinned build

The build we target is recorded in `docs/re/pinned-build.md`. Every copy comes from the person's own legitimately obtained client.

- **From your own install** (Steam or Gameforge's launcher). Copy it out of the library, so nothing else touches it and the sandbox can bind it; the sandbox refuses Steam directories:

  ```bash
  mkdir -p "$FENJI_CLIENT_PATH"
  cp -a --reflink=auto "<library>/steamapps/common/Swords of Legends Online/." "$FENJI_CLIENT_PATH/"
  ```

- **With DepotDownloader**, if SoLO is in your Steam library. You type your credentials into DepotDownloader; Fenji never sees them:

  ```bash
  DepotDownloader -app 1418100 -depot 1418101 -manifest 5002037851744157048 \
    -os windows -osarch 64 -username <your-steam-login> -dir "$FENJI_CLIENT_PATH"
  ```

Then compare `bin64/gujianol.exe`'s SHA-256 with `docs/re/pinned-build.md`. On the Mac, copy the same files for static analysis.

## Analysis setup

- **REA MCP server:** `rea-agents` (version pinned in `scripts/versions.env`), registered in `.mcp.json` through `scripts/rea-mcp.sh`.
- **Native engine:** **Ghidra 12.1.4** from the official release zip. Bootstrap installs it to `~/.local/share/fenji/tools/ghidra_12.1.4_PUBLIC`, and `~/.config/fenji/env.sh` exports `GHIDRA_INSTALL_DIR` and `JAVA_HOME` (JDK 21).
- **Role split:**
  - The Ghidra GUI project (`re/ghidra/fenji.gpr`, git-ignored) is the main analysis surface for `gujianol.exe`, and the canonical place for names, types and structs. The owner and Claude share it through the `ghidra` MCP server (the GhidraMCP extension plus the `bridge-mcp-ghidra` bridge to `127.0.0.1:8089`).
  - REA is the evidence ledger: Evidence IDs, unknowns, decompilation and verification. For `gujianol.exe` it analyses the whole ~52,000-function executable once per session (about four minutes before the first answer, then seconds), and it doesn't see the GUI project's names and types, so it isn't the exploration surface. See [Evidence for `gujianol.exe`](#evidence-for-gujianolexe).
  - Enable the plugin once in the GUI (File → Configure → Miscellaneous → GhidraMCP); `curl -s http://127.0.0.1:8089/check_connection` should then answer.
- **Readiness check** (changes nothing): `npx -y rea-agents@<version> doctor --client claude_code --json`.

## Evidence for `gujianol.exe`

Settled in M2 (issues #58 and #18, 2026-10-12) by running REA on the pinned exe and timing it. The timings are from the Fedora box (32 cores, 62 GB); the Mac hasn't been measured. Tags as in [spike-0](spike-0.md): **Observed**, **Inferred**, **Unknown**.

### The norm

For a new finding about `gujianol.exe`, read the function through REA (recipe below) and cite its Evidence ID next to the pinned-exe address. Pinned-exe addresses from the shared Ghidra project stay acceptable on their own: for everything written before M2 (nobody back-fills), for what only the GUI project holds (the owner's names, types and structs), and for code REA can't reach (see Pitfalls). Say in the finding which tool the reading came from.

Why:
- **The cost is per session, not per function.** On the Fedora box REA's only analysis provider is Ghidra headless (Hopper and IDA report `unavailable`). `open_binary` takes a few seconds and starts nothing. The first query of the session then imports the exe into a temporary read-only project and waits for Ghidra's complete default auto-analysis. There is no lighter preset (`analyzer_preset: ghidra-default`) and no way to analyse one function only. That first query took 222 s, and at most 225 s in a second fresh session, with a Java process of about 2 GB. Every later query answered within seconds. **Observed**
- **IDs are reproducible.** An Evidence ID is `ev_` plus a SHA-256 over the whole record: the exe's hash, provider and version, analysis profile, query and result. `procedure_pseudo_code` on 0x14029C330 returned the same ID in two independent sessions, so anyone can check a cited ID by repeating the exact call, without our evidence bundle. The ID covers the call's parameters exactly as sent: `0x14029C330` and `0x14029c330` give different IDs. **Observed**
- So "targeted" was the wrong word for REA on this exe: it always analyses all of it. What it costs is about four minutes of waiting, once per session.

### Recipe

To check a new setup (REA 6.3.0, Ghidra 12.1.4): step 3's call, exactly `procedure_pseudo_code {procedure: "0x14029C330"}` (upper-case hex), must return `ev_2487ebf9c2001ca037d52f138681f91175ece7bc1de0ba6422232f62d2ca9b77`.

1. `binary_session {}`: is anything open? The reply is about 200 KB, so the harness saves it to a file; read `open` from it with `python3 -I` or `jq`. If it is true, `close_binary {}` first.
2. `open_binary {path: "<absolute $FENJI_CLIENT_PATH>/bin64/gujianol.exe"}`: an absolute path and nothing else. On a machine where Hopper or IDA is installed, add `provider_id: "ghidra"`: automatic selection could pick another provider, and the provider is part of every Evidence ID. The reply has the `sha256`; compare it with [`pinned-build.md`](pinned-build.md).
3. Ask the first question and wait about four minutes. Any query starts Ghidra, so make it one you need, such as `procedure_pseudo_code {procedure: "0x14029C330"}`. Don't send another request meanwhile, and don't cancel it: a cancelled request throws the analysis away and the next query pays again.
4. Ask the rest in the same session. Each of these returns an Evidence ID. A procedure is `0x` plus hex, or the `FUN_…` name in lower case. Type addresses as REA prints them, in lower case: the case is part of the ID (see How to cite).
   - `procedure_pseudo_code {procedure}`: the decompiler text, about 10 KB, inline. The default read.
   - `search_strings {pattern, mode: "literal" | "regex"}`, then `xrefs {address}`: from a string to the code that uses it. `xrefs` needs an explicit address and answers with instruction addresses.
   - `procedure_callers {procedure}`, `procedure_callees {procedure}`, `procedure_references {procedure}`: call edges and typed references.
   - `analyze_function {procedure}`: the whole dossier (pseudocode, assembly, callers, callees, typed references, referenced strings, basic blocks). It ran from 0.1 MB to 4.6 MB for the functions tried, so it always ends up in a file: pull fields out with `python3 -I` or `jq`, and don't read it whole.
   - `record_unknown` for what stays open.
5. `export_evidence_bundle {path: "<abs>/re/<topic>/rea/bundle-<n>.json"}` before you close. It won't overwrite, so use a new name each time. Bundles stay in `re/`.
6. `close_binary {}`, on every path including errors. The temporary project goes with it. `binary_session {}` should then say `open: false`.

### Pitfalls

- **The deadline.** The REA server gives Ghidra's import and analysis 330 s by default. 222 s fits with a third to spare; a slower machine may not. So `scripts/rea-mcp.sh` raises it to 600 s (`REA_GHIDRA_STARTUP_TIMEOUT_MS=600000`, milliseconds) unless the variable is already set. If the first query still fails with a startup timeout, start Claude Code with a larger value in its environment and restart the REA server. Don't put it in `~/.config/fenji/env.sh`: `scripts/bootstrap.sh` regenerates that file. **Unknown**: nobody has hit the limit, so this comes from the package's source and README, not from a failure.
- **One target per session.** Close the exe before you open anything else.
- **Snapshots.** `close_binary {snapshot_path}` saves the answers so far and `open_binary {path, snapshot_path}` replays them without starting Ghidra (2 s for a replayed `analyze_function`). Only `analyze_function` and calls that pass a `document` argument are cached, any other query still pays the four minutes, and a replayed record gets a different ID because it carries an extra limitation. A snapshot is for re-reading, not for citing.
- **What REA can't reach.** It reads only what Ghidra's auto-analysis made into a function. Where there is none, the call fails with "No procedure exists at the requested address" and returns no ID. Example: the callback at 0x14068EFC0 (below). Cite the address.
- **Names stay in the GUI project.** REA's `set_address_name` and comment tools edit only the temporary project. REA doesn't see the GUI project's names and types, so its decompiler text can name things differently from what the owner sees.

### How to cite

- Name a function by its Ghidra name and address: `FUN_14029C330` (upper-case hex here; Ghidra and REA print lower case). Where there is no function, the address alone: 0x14068EFC0.
- Put the full ID in the doc's evidence index, in the row of the evidence key the findings point to, next to the exact call that produced it: `FUN_14029C330`, `procedure_pseudo_code {procedure: "0x14029C330"}`, `ev_2487ebf9c2001ca037d52f138681f91175ece7bc1de0ba6422232f62d2ca9b77`. The ID depends on the parameters exactly as sent, hex case included, so the call is what makes it checkable. Write addresses in calls in lower case, as REA prints them; the setup check above is the one upper-case exception. Findings keep citing the key; doc comments in code keep citing the doc section and the `FUN_` name.
- Cite the narrowest query that supports the claim: `procedure_pseudo_code` for a reading of code, `search_strings` plus `xrefs` for "this string is used here", `procedure_callers` or `procedure_callees` for call edges. An ID proves what the tool returned, not our reading of it, so the tag still applies.
- Cite IDs from live queries, not from a snapshot replay.
- IDs change with the exe, REA or Ghidra (pinned in `scripts/versions.env`: REA 6.3.0, Ghidra 12.1.4). After a bump, repeat the queries and update the IDs. The address is the stable key.
- Bundles and snapshots stay in `re/`. An ID is checked by repeating its exact call.

### Naming functions from `AurogonEngine.dll` (spike-0 1.8)

**Answer: partly.** The exports name engine functions that `gujianol.exe` contains. They name none of its network code.

| # | Finding | Tag | Evidence |
|---|---|---|---|
| D1 | `tools/GujianOL_Editor/AurogonEngine.dll` (20,723,712 bytes, SHA-256 `eb998b17055dae34fb737e5787f7c35f0aa8ba18f87ec815b17cd0c336d55a4d`, image base 0x180000000) exports 3,209 names. Every export has a name: none is ordinal-only or forwarded. 3,086 are MSVC-decorated C++ names, which carry the class, the function and the parameter types; 123 are plain C names. The exception table has 54,670 entries. This confirms spike-0 1.7. | **Observed** | the PE export table, read with a stdlib-only script |
| D2 | By name, 1,839 exports are engine or game code (`CutScene`, `Character`, `Fox`, `Horn`, `EnvParams`, `SoundManage`, `VFX_*`, `Hasher`, `vfs_hook_*` and more), 887 are Dear ImGui, 337 pugixml, 122 NVIDIA (Blast, NGX) and 24 `std`. | **Observed** | the same names, demangled with `llvm-undname` |
| D3 | No export is a network or cipher function. A name search for socket, packet, client, server, login, cipher and similar words finds only `GetLoginSetting` and `IsLoginScene`, which concern the login scene's settings. So the DLL names nothing in the transport code of [`transport.md`](../protocol/transport.md). | **Observed** | the same names |
| D4 | The exe contains the same compiled engine code (the DLL isn't in `bin64/`). 152 of the 2,494 exports of 24 bytes or more have a byte-identical copy in the exe's `.text` (148 with exactly one hit): 50 engine or game, 91 ImGui, 8 NVIDIA, 3 pugixml. A function with a call or an RIP-relative operand can't match byte for byte, so this is a lower bound. | **Observed** | a byte search of each DLL function (extent from its exception table) in the exe |
| D5 | 254 exports load a string of 8 or more characters that the exe also contains (169 of them engine or game). Strings find functions that aren't byte-identical (rows 2 and 3 below). | **Observed** | a scan of each function for RIP-relative loads of strings, then `search_strings` and `xrefs` |
| D6 | How many exe functions the exports could name in all is **Unknown**. The byte search skips every function with a relative operand, and nobody has matched those yet. | **Unknown** | |

Three matches. The exe side was read through REA; each ID is given with the exact call that produced it.

| DLL export (RVA, size) | `gujianol.exe` | What matches | Tag | Evidence |
|---|---|---|---|---|
| `Hasher::MurmurHash2(void const *, int, unsigned int)` (0x2C6A10, 142 bytes) | FUN_1408ECC50 (142 bytes; callers FUN_1407EFD00 and FUN_140A33820) | All 142 bytes are identical, and the exe has no other copy. The code is the reference MurmurHash2 (multiplier 0x5BD1E995, shifts of 24, 13 and 15). | bytes **Observed**; same function **Inferred** | `analyze_function {procedure: "0x1408ecc50"}` → `ev_86996e4719acb0a240a8d666382fa0df9446eabf50c3a7d65b06a0cca247f84e` |
| `Fox::FoxManager::LoadFoxModelTable(void)` (0x202160, 7,149 bytes) | FUN_140707CC0 (5,328 bytes) | Both reference the same 25 distinct strings, first used in the same order (56 references in the exe), among them a `tables/present/` table name and `EnhanceNumber`. The exports `GetFoxInfoByTableID`, `GetFoxDecoInfoByTableID` and `IsTexValid` have byte-identical copies at 0x140707780, 0x140707680 and 0x1407079B0, just before it. The functions differ in size (7,149 and 5,328 bytes), so the match rests on the strings. | strings **Observed**; match **Inferred** | `analyze_function {procedure: "0x140707cc0"}` → `ev_3214fa1d2d390da0bd0525b3dd81d7e469752b9a4d41f2b917c73f837ad3b282` |
| `vfs_hook_init(…)` (0x2D3570, 2,775 bytes) | FUN_14068FB60 (2,011 bytes; the archive set-up of [`m1-script-format.md`](m1-script-format.md) G5) | Both read `--fs=` and share 9 distinct strings (the exe also uses `patch`, the DLL three `_index` strings); from `tools` to `vForge` their 15 references come in the same order, among them `data/vision/base`, `vision/base` and `asset/vForge`. The exe zeroes five buffers (0x80, 0x100, 0x100, 0x1000 and 0x1000 bytes), the DLL four of those sizes (0x80, 0x100, 0x100 and 0x1000). Both call one helper five times (DLL 0x2D3100, exe FUN_14068F640), which is the same code in both apart from one import call. Their instruction streams are about 60% similar once addresses are normalised. The DLL's version takes the archive functions as parameters; the exe's calls them directly. | strings and sizes **Observed**; match **Inferred** | `analyze_function {procedure: "0x14068fb60"}` → `ev_0ec2f3591ed130397871ee4049fe136d24e26db7c3608381bb377b9b1bac7244`. String route: `search_strings {pattern: "asset/vForge"}` → `ev_12afc993ea91cb91fe394bce5f6c2aebff0a61255835fe99d5b5fdffede8e668`, then `xrefs {address: "0x142da5b60"}` → `ev_b19fa9874bfdef1222912d4824616231390c683d113461051c6c7444b3b84a93` |

The third match also names something the exe leaves unnamed. The export `vfs_hook_zip()` returns a callback (DLL 0x2D26D0), and its type shows two structure names, `_ZFSArchives` and `_KSVZBlock`. The exe has the same callback at 0x14068EFC0, with the same 51-instruction skeleton: a four-way switch on a codec id from 0 to 3 that returns a name (Kraken, Mermaid, Selkie or Leviathan, for ids 0 to 3) and two function pointers. Neither REA nor the GUI project has a function there (the case under Pitfalls). **Observed**. That it is the callback `vfs_hook_init` passes on is **Inferred**. How the ids 0 to 3 relate to the codec byte of spike-0 6.6 is **Unknown**.

How to repeat it:
- Read the export table with a small stdlib-only PE reader run as `python3 -I`. REA's `inspect_binary_layout` needs pwntools and reads only ELF files here, and REA's function tools would start another whole-DLL analysis (54,670 exception-table entries are about as many functions as the exe has).
- Demangle with `llvm-undname`. Take each function's extent from the exception table and search its bytes in the exe's `.text`. For the rest, find the strings it loads and take them to `search_strings`, `xrefs` and `procedure_pseudo_code`. Compare shapes with `objdump -d` on the DLL, with addresses normalised.
- Keep the DLL out of the shared Ghidra project, which is shared live with the owner; that rules out `find_similar_functions_fuzzy`, which needs both programs open in it.
- The scripts are in `re/m2/rea/` and aren't committed.

## Running the client

Only inside `scripts/spike/isolated-run.sh` (design spec §2 rule 4), except the guarded bare-metal play tests on the Linux game box that CLAUDE.md describes (owner's decision, 2026-10-13). Inside the sandbox:
- the network is loopback only, and names come from a hosts file;
- there are no host daemon sockets;
- the display is a nested X server (`--x11 N`);
- the client runs non-root (`--no-root`); sandboxed runs start GE-Proton's own `proton` script directly, because umu's runtime container fails inside the sandbox (`bwrap: Can't find source path /sys`, seen without `--cpu-topology`; umu with it is untested);
- Wine/Proton runs need `--cpu-topology`: Wine builds its processor information from `/sys/devices/system/cpu`, and without it the client hangs at start.

Before a run:
- Quit Steam; the wrapper refuses to start while it runs.
- Bind only directories that belong to the run.
- Run the stub on loopback inside the same sandbox. When the auth can carry a real platform ticket (a Steam or Gameforge login rather than `--gf-sdk=mocked`), pass `--stub-arg --speak --stub-arg --no-auth-capture`: no stub file then keeps the auth (#102). The client's own logs are another matter.

Read the wrapper's header before the first run.

The client flags that reach the login scene in a dev run are in [spike-0 §8](spike-0.md#8-runtime-observation-isolated-stub-only) (`--gf-sdk=mocked` is dev-only, gate #11). Run the client with `scripts/spike/run-client.sh setup` once, then `run-client.sh run NAME … -- ARGS`; it adds fenji-stub and the AGT driver watchdog. Exit 3 means the watchdog saw a sign of driver activity and stopped the run: stop all runs and report. The script's header lists every exit status, and the watchdog's lists the signs.

From M3, a run can use Fenji's own server and the launcher's ticket instead of the stub (`cargo build --release -p fenji-server -p fenji-launcher` first):
- `--server CONFIG` runs `fenji-server` with a copy of CONFIG (a `fenji.toml` whose `login_bind` and `api_bind` are on 127.0.0.1) and waits for both ports. Its log is the run's `server.log`. A relative `[store] path` stays in the copy's folder: a fresh database each run.
- `--server-data DIR` keeps the database between runs. DIR is a directory under `re/runs/` that holds no link or special file, and the config's `[store] path` must be absolute and inside it. The sandbox can write DIR, so open it on the host only before the first run (to create the account), or as a copy made with `watchdog.py collect`.
- `--launch "ARGS"` runs `fenji-launch play --print ARGS` once the server is up, and needs `--launch-password FILE`. The file reaches the sandbox only on its standard input: it is never bound, copied, recorded, or named in an argument or the evidence. The client gets only the `gj_*` pairs, and `fenji-launch` may print nothing else. `io/launch.txt` records the pairs with the ticket redacted, and any Fenji ticket found in the evidence afterwards is redacted in place and listed in `redactions.txt`.
- `--key T:KEY` and `--type T:TEXT` press a key (an X keysym name such as `Return` or `space`) or type printable ASCII T seconds into the run, as `--click` clicks (`xkey.py`, through XTEST). Presses run in time order, none after the timeout.

If the server isn't ready or the launcher fails, the client isn't started and the run exits 7.

### Bare-metal runs

`scripts/spike/run-bare.sh` runs the client the way a player starts it, under every guard of CLAUDE.md's "Bare-metal client runs" (#139):
- **As a player starts it:** `fenji-launch play` checks a reflink copy, made for the run, of the pinned copy at `$FENJI_CLIENT_PATH`. That copy is refused if it is a link, holds a link, or sits in a Steam library. `fenji-launch` then prepares the dedicated prefix `re/bare-metal/compat` with `umu-run createprefix`, signs in, and starts the client through umu-run and the pinned GE-Proton. No SDK flag is ever passed.
- **No Steam or Wine:** neither may be running. Steam is checked again every 2 s during the run, and Steam starting stops the run.
- **One shared namespace:** `fenji-server` and the client run in the same one. It keeps the real files, home, GPU, audio and display. But `/run`, `/tmp` and `/dev/shm` are empty tmpfs, bar the user's audio sockets and the display's X11 socket, so no host D-Bus, systemd-resolved, portal or agent socket is there. IBus's private sockets in `~/.cache/ibus` are hidden behind an empty tmpfs too. Other sockets elsewhere in the real filesystem stay reachable, and so does the audio daemon. That is the accepted residual: its pulse server takes `load-module`, so the client could load a network module on the host (`module-tunnel-sink`, `module-native-protocol-tcp`; #148). The network is loopback only, and isolated-run.sh's seccomp filter blocks key requests. The client's network reaches the Fenji server only; `test-run-bare.sh` checks that from inside, where it can make the namespace. `--desktop` loosens this (below).
- **Its own `/proc`:** the namespace needs one. Without it, umu's pressure-vessel looks its child up in the host's `/proc` by a namespace PID and hangs.
- **The watchdog:** it arms on that PID namespace before anything starts. S1 scans `re/bare-metal` (the prefix and the run's copy) and the namespace's own `/run`, `/tmp` and `/dev/shm`, through `/proc/PID/root`, less the sockets and the auth file bound into them. It scans nothing else of the host's files, whose own Proton prefixes would trip it. S4 reads the prefix's registry directly, not through umu's `pfx` link. S2 and S4 work as in sandboxed runs, and S3 follows GE-Proton's logs, which it recreates for each session. The evidence also lists any new `*.sys` file under umu's runtime and `/var/tmp`.

Run `run-bare.sh setup --desktop` (or `setup --display N`) once. It builds the prefix with `umu-run createprefix`, then primes it: it starts `cmd.exe` once through Proton's launcher (`umu.exe`), which registers GE-Proton's Discord bridge service. That puts the service in the watchdog's baseline. Unprimed, the client's first launch registers it, and the watchdog stops the run (bm-1, 2026-10-13). The prime's mark names the GE-Proton build. A run refuses an unprimed prefix, or one primed by another GE-Proton build, since a new build upgrades the prefix, and `setup` primes an existing one. Then run `run-bare.sh run NAME --server CONFIG --server-data DIR --username USER --password-file FILE` with either `--display N` or `--desktop`; a run has no default display. `--key`, `--type`, `--click` and `--timeout` work as in `run-client.sh`. CONFIG must bind 127.0.0.1 and name 127.0.0.1 as its `public_host`, and DIR is a directory under `re/bare-metal/`. The evidence goes to `re/bare-metal/out/NAME`. The exit statuses are `run-client.sh`'s, plus 1 when Steam started during the run, unless the watchdog stopped the run or failed: exit 3 or 4 wins, and the Steam note is printed too.

The display:
- **A nested display (`--display N`)** must be a dedicated rootful Xwayland, Xephyr or Xvfb. The runner makes `isolated-run.sh`'s `--x11` checks, bar its GPU and shared-memory rules. It refuses the session's own display (DISPLAY, by number), a display the compositor locks, a `-rootless` server, one started with `-enable-ei-portal`, and a libXfont with font-server support. It also refuses a socket directory that isn't sticky, and a socket or lock that isn't yours or is the wrong kind of file. Run nothing else on that display: with `-ac`, a terminal there is a shell outside the namespace. The real GPU can present to it only with glamor on, so start a dedicated one without `-shm`, for example `Xwayland :98 -geometry 1920x1080 -ac -nolisten tcp -noreset -extension MIT-SHM &`, and pass `--display 98`. Never share it with sandboxed runs: `isolated-run.sh`'s `:99` keeps glamor off, so that a sandboxed client gets no GPU device. With glamor off, the real GPU's first frame ends the client (bm-2). Screenshots and key presses reach the client there and nothing else.
- **`--desktop` (`setup` takes it too)** runs the client on your session's own display, as a player has it.
  - The display's auth file comes in read-only, and the run takes no screenshots and sends no key presses or clicks, since they would take or reach the whole desktop.
  - The client then joins the session's X server, where it can see, read and type into other X11 windows, and read and replace the clipboard the compositor bridges into X11. Its XTEST input may also reach native windows through the compositor (libei), a terminal included.
  - The namespace's own network stays loopback-only: the risk is the X server and the compositor as a way out of it. So a desktop run guards the client less strictly than a nested display does.
  - The owner accepted that for hands-on test runs (2026-10-13). Players run the game on their own desktop anyway.

## Working rules

- Static analysis of files on disk comes first. Runtime observation (Wine `WINEDEBUG`, the client's own log, stub sessions) happens only inside the sandbox, against our own stub or server, or in the guarded bare-metal play tests CLAUDE.md describes.
- Every conclusion distinguishes observation, inference and unknown, and cites evidence as set out in [Evidence for `gujianol.exe`](#evidence-for-gujianolexe). Unresolved questions go into REA with `record_unknown`.
- Decision gates (client patching, circumventing protection, the embedded kernel driver, Oodle licensing) stop the work until the owner decides.
- Close REA sessions (`close_binary`) when an investigation is finished. Never overwrite an evidence bundle; export to a new name.
- Community sources must be clearly licensed with clean provenance. List each in `docs/re/sources.md` with its licence and what we used it for. Rejected sources are listed there too, and anyone who has read one does no RE or protocol work.
