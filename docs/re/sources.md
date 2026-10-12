# External sources

Every outside source we take hints from (design spec §2 rule 2, §4 C). Only clearly licensed material with clean provenance: nothing derived from leaked server files, binaries, scripts or source.

## Used

| Source (URL) | Licence | Provenance checked | What we used it for | Date |
|---|---|---|---|---|
| [Lua 5.3](https://www.lua.org) (vendored by [mlua](https://github.com/mlua-rs/mlua) 0.12.2) | MIT | Yes: upstream Lua, built from source by the `lua-src` crate | Test-only: compiles our own Lua source into stock 5.3 bytecode fixtures at test time (M1 Task 4) | 2026-10-12 |
| [unluac](https://sourceforge.net/projects/unluac/) `unluac_2025_12_23.jar` | MIT (project page) | Yes: a general Lua 5.0–5.4 decompiler by its author; nothing SoLO-specific | External tool: decompiles our normalised copies of the client's own scripts; output only in `re/` (M1 Task 7) | 2026-10-12 |
| [LLVM](https://llvm.org) 22 `llvm-undname` (Fedora 44's `llvm-22.1.8`) | Apache-2.0 WITH LLVM-exception | Yes: the upstream toolchain as Fedora packages it; nothing SoLO-specific | External tool: demangled the export names of the client's own `AurogonEngine.dll` ([`README.md`](README.md#naming-functions-from-aurogonenginedll-spike-0-18), spike-0 1.8) | 2026-10-12 |
| [GNU binutils](https://www.gnu.org/software/binutils/) 2.46 `objdump` (Fedora 44's `binutils-2.46.1`) | GPL-3.0-or-later | Yes: the upstream toolchain as Fedora packages it; nothing SoLO-specific | External tool: disassembled the client's own `AurogonEngine.dll` to compare function shapes with the exe ([`README.md`](README.md#naming-functions-from-aurogonenginedll-spike-0-18), spike-0 1.8) | 2026-10-12 |
| [GE-Proton](https://github.com/GloriousEggroll/proton-ge-custom) GE-Proton11-7 | BSD-3-Clause (Proton) with Wine's LGPL and other bundled licences | Yes: a public Proton fork | External runtime for sandboxed client runs, never redistributed (M1 Task 9) | 2026-10-12 |

## Rejected

Material refused because it is leaked server material, or derives from it. Nobody doing Fenji RE or protocol work may read it.

| Source | Why rejected | Exposure | Date |
|---|---|---|---|
| "GuJian Online (古剑奇谭网络版) Server Reverse Engineering Report" (`GuJianOL-Server-RE-Report.md`, distributed in a RaGEZONE zip) | Its table of contents shows an analysis of the game's server package (server binaries, start script, server-side scripts), which the developer never published | One Claude session, `018pTqyt…` (the Caphras → Fenji transition), read its table of contents only and used nothing. That session coordinated Fenji until about 23:00 AEDT on 2026-10-11, making process rulings on clean agents' evidence, and did no RE or protocol work apart from one diagnostic sandboxed start of the client (22:49 on 2026-10-11, [spike-0](spike-0.md) P0, not used for any finding). Coordination then passed to `014j8e76…`, a session that has not read it. The owner deleted the downloaded copy on 2026-10-11 | 2026-10-11 |
