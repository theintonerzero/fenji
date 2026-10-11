# M1: how much server-side logic the client's scripts carry

The client's combat scripts carry a lot of logic that runs only on the server. Of the 2.39 million instructions under `data/scripts`, 17.6% run only behind an `IsServer()` check, against 12.3% only behind `IsClient()`. The client also ships two directories of scripts that are entirely server-side, `data/scripts/combat/server` and `data/core/combat/server`. Everything outside `data/scripts` has no such checks at all.

## Method

`luatool census` (M1 plan Task 11) parses every Lua chunk embedded in the pinned `gujianol.exe` (build 10015727) and measures each function's bytecode for one shape: the classic `if IsServer() then … end`. The shape is these four instructions in a row:
1. a read of the global `IsServer` or `IsClient` (`GETTABUP`, or the client's `TRYTABUP`, `TRYLIBUP` or `XGETTABUP`);
2. a call with no arguments and one result (`CALL A 1 2`);
3. a `TEST` of that result;
4. a forward `JMP`.

The instructions the `JMP` skips run only when the check passes: on the server for `IsServer()` or `not IsClient()`, and on the client for the other two. When that block is a lone `RETURN` (`if not IsServer() then return end`), the rest of the function runs only on the other side.

| Column | Meaning |
|---|---|
| chunks, functions, instructions | What was measured |
| reads_is_server, reads_is_client | Functions that read the global `IsServer` / `IsClient` |
| server_only | Instructions that run only on the server, by the shape above |
| client_only | Instructions that run only on the client, by the shape above |

Limits: it counts instructions, not behaviour, and it misses other shapes. Those include a check kept in a local (`local s = IsServer()`), checks inside expressions (`a and IsServer()`), and logic that a file runs only because the server loads it, which is why the `server/` directories are listed separately. Evidence: `re/m1/census/census.tsv` (per area) and `re/m1/census/census-server-chunks.tsv` (per chunk), git-ignored. **Observed**: the counts. **Inferred**: what they mean.

## Results by area

By embedded directory:

| Directory | Chunks | Functions | Instructions | Read `IsServer` | Read `IsClient` | Server-only | Client-only |
|---|---|---|---|---|---|---|---|
| `data/scripts` | 18,855 | 59,518 | 2,394,714 | 11,354 | 13,113 | 420,322 (17.6%) | 294,019 (12.3%) |
| `data/interface` | 1,006 | 25,674 | 1,175,001 | 0 | 0 | 0 | 0 |
| `data/core` | 515 | 14,336 | 469,481 | 0 | 0 | 0 | 0 |
| `data/locale` | 67 | 937 | 55,755 | 0 | 0 | 0 | 0 |
| all | 20,443 | 100,465 | 4,094,951 | 11,354 | 13,113 | 420,322 (10.3%) | 294,019 (7.2%) |

`data/scripts/move_formular` holds 707,331 of those instructions in 32 chunks with no checks; without it, `data/scripts` is 24.9% server-only and 17.4% client-only. The main areas of `data/scripts`, by server-only instructions (shares of each area's instructions):

| Area | Chunks | Instructions | Server-only | Client-only |
|---|---|---|---|---|
| `combat/buff` | 5,826 | 569,859 | 170,872 (30.0%) | 91,781 (16.1%) |
| `combat/skill` | 6,037 | 641,586 | 155,312 (24.2%) | 142,269 (22.2%) |
| `combat/effect` | 3,966 | 214,206 | 66,678 (31.1%) | 21,736 (10.1%) |
| `combat/dummy` | 1,294 | 91,450 | 14,127 (15.4%) | 31,507 (34.5%) |
| `combat/passive` | 1,207 | 81,880 | 10,001 (12.2%) | 731 (0.9%) |
| `combat/projectile` | 196 | 8,120 | 1,269 | 1,473 |
| `combat/default` | 11 | 15,739 | 1,342 | 4,052 |
| `combat/server` | 112 | 29,209 | 0 | 0 |
| `emotionaction` | 89 | 12,898 | 8 | 0 |
| `actions` | 48 | 8,475 | 0 | 0 |
| `move_formular` | 32 | 707,331 | 0 | 0 |

## Server-side areas

`census-server-chunks.tsv` lists 303 chunks.

**Directories named `server/`: 120 chunks.**
- `data/scripts/combat/server`: 112 chunks, 29,209 instructions.
- `data/core/combat/server`: 8 chunks, 22,087 instructions.

They hold no `IsServer` checks because the whole file is server-side. The largest are in `data/core/combat/server`:

| Chunk | Instructions |
|---|---|
| `combat_system` | 11,211 |
| `combat_record_analyser` | 4,421 |
| `combat_statistic` | 2,789 |
| `combat_crowd_control` | 1,689 |
| `combat_skill_version_upgrade` | 1,394 |

**Chunks at least 90% server-only by the shape: 183 chunks, 32,926 instructions, 30,632 of them server-only.**

| Area | Chunks |
|---|---|
| `combat/effect` | 73 |
| `combat/skill` | 54 |
| `combat/buff` | 54 |
| `combat/passive` | 1 |
| `combat/dummy` | 1 |

## What this means for M5

**Inferred:**
- **The combat rules are largely in the client's own scripts.** Skills, buffs and effects carry their server-side branches (about 390,000 server-only instructions in those three areas alone). The `server/` directories add the server's combat system itself. This favours the parent spec's hybrid rule (§6.4): the Fenji server could run these scripts in its own Lua VM, with native code supplying the engine functions they call, rather than reimplementing combat from scratch.
- **The rest holds little or nothing for the server.** The interface, core client modules and locale data are client-side. `move_formular` is large but unguarded, which suggests shared data (movement formulas) that both sides need.
- **What M5 would need in addition:** the engine API these scripts call on the server (what `IsServer()` code calls), the server's own Lua entry points, and tables from the archives (behind gate #39).

## Unknown

- How much server logic hides in shapes the measure misses: locals, expressions, dispatch by caller.
- Whether the shipped `server/` scripts are complete, or a subset the client also loads for prediction.
- Which engine functions the server-only branches call, and how many of those exist only in the real server binary. That's the census of M5's native work, and it's not measured yet.
