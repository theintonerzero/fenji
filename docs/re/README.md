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

`scripts/check-forbidden-files.sh` (run in CI and by the pre-commit hook) rejects game binaries, archives, Lua bytecode, captures and analysis databases.

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
  - REA is the evidence ledger: Evidence IDs, unknowns, targeted decompilation and verification. Its full auto-analysis of the ~52,000-function executable is too slow to be the main surface.
  - Enable the plugin once in the GUI (File → Configure → Miscellaneous → GhidraMCP); `curl -s http://127.0.0.1:8089/check_connection` should then answer.
- **Readiness check** (changes nothing): `npx -y rea-agents@<version> doctor --client claude_code --json`.

## Running the client

Only inside `scripts/spike/isolated-run.sh` (design spec §2 rule 4). Inside it:
- the network is loopback only, and names come from a hosts file;
- there are no host daemon sockets;
- the display is a nested X server (`--x11 N`);
- the client runs non-root (`--no-root`); sandboxed runs start GE-Proton's own `proton` script directly, because umu's runtime container fails inside the sandbox (`bwrap: Can't find source path /sys`, seen without `--cpu-topology`; umu with it is untested);
- Wine/Proton runs need `--cpu-topology`: Wine builds its processor information from `/sys/devices/system/cpu`, and without it the client hangs at start.

Before a run:
- Quit Steam; the wrapper refuses to start while it runs.
- Bind only directories that belong to the run.
- Run the stub on loopback inside the same sandbox.

Read the wrapper's header before the first run.

The client flags that reach the login scene in a dev run are in [spike-0 §8](spike-0.md#8-runtime-observation-isolated-stub-only) (`--gf-sdk=mocked` is dev-only, gate #11). Run the client with `scripts/spike/run-client.sh setup` once, then `run-client.sh run NAME … -- ARGS`; it adds fenji-stub and the AGT driver watchdog. Exit 3 means the watchdog saw a sign of driver activity and stopped the run: stop all runs and report. The script's header lists every exit status, and the watchdog's lists the signs.

## Working rules

- Static analysis of files on disk comes first. Runtime observation (Wine `WINEDEBUG`, the client's own log, stub sessions) happens only inside the sandbox, against our own stub or server.
- Every conclusion distinguishes observation, inference and unknown, and cites evidence: REA Evidence IDs, or pinned-exe addresses where REA can't reach. Unresolved questions go into REA with `record_unknown`.
- Decision gates (client patching, circumventing protection, the embedded kernel driver, Oodle licensing) stop the work until the owner decides.
- Close REA sessions (`close_binary`) when an investigation is finished. Never overwrite an evidence bundle; export to a new name.
- Community sources must be clearly licensed with clean provenance. List each in `docs/re/sources.md` with its licence and what we used it for. Rejected sources are listed there too, and anyone who has read one does no RE or protocol work.
