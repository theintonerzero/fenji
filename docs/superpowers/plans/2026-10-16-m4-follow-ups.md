# M4 follow-ups: Implementation Plan

> **For agentic workers:** REQUIRED: use superpowers:subagent-driven-development for the subagent tasks and superpowers:executing-plans for the inline ones. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** fix what M4's reviews filed for later, in patch releases after v0.5.0. The owner decided on 2026-10-16 to release v0.5.0 before M4's acceptance session, with follow-ups as 0.5.1, 0.5.2 and so on. M4's milestone closes when they're done.

**Requirements:** the issues themselves. Each review finding there names its fix, and the issues are binding where this plan is silent:
- #348: the launcher's final review's deferred minors;
- #359: hosting's follow-ups;
- #281: the world's final review's follow-ups;
- #282: back to character select;
- #244: the two-client runners and watchdog;
- #174: the v0.4.1 review's last two runner items.

The specs that set the behaviour stay binding: `docs/superpowers/specs/2026-10-13-m4-world-design.md`, `…/2026-10-14-m4-hosting-design.md` and `…/2026-10-15-m4-launcher-design.md`.

## Global Constraints

- **Ground rules** (CLAUDE.md):
  - every task but T8 and T9 is repository-only, with nothing from `re/` or the client;
  - T8 and T9 need sandboxed client runs, under `scripts/spike/run-client.sh` and every guard;
  - the clean room holds.
- **The gate before each PR:** `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny check`, `scripts/check-forbidden-files.sh` and `scripts/check-unsafe-allowed.sh`. For the launcher, also its own checks (`docs/development.md`, "The desktop launcher").
- **Commits and PRs:** Conventional Commits with exactly one `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` trailer, and never a session link. One PR per task, linking its issue items. Test-first for every behaviour fix.
- **Players' text is untrusted** (hosting Review Focus 1): every string the console, the launcher or a log shows goes through its `clean`.
- **The terminal is given back on every exit** (hosting Review Focus 2).

## Releases

- **0.5.1:** T1, T2 and T3, what M4's acceptance session will touch first.
- **0.5.2:** T4 to T7, robustness, tests, docs and tooling.
- **0.5.3:** T8 and T9, what needs the client.

Each release follows `docs/development.md`'s steps, and the owner approves its `sign` job.

## Tasks

### Task 1 (inline): the launcher's player-facing minors (#348)
Every item of #348 except the rehearsals at its end, which go to the acceptance session:
- one game at a time across a launcher restart, through a pid file or a wineserver check;
- ChooseFolder, Download and Repair refused while the game starts or runs;
- the Servers row's PLAY only when `playVariant(s) === "ready"`;
- "Remember this server" honoured for every server;
- the view subscribes before its first fetch;
- the no-terminal path's text and Repair's manual path;
- Cancel stops "Getting DepotDownloader…";
- the server's name cleaned;
- probes keep focus;
- DismissNotice clears only its own notice;
- a repair's partial check recorded as such;
- depot run folders cleaned;
- `start()`'s error shown in a dialog;
- starting has a cancel and a time limit;
- a guard against rolling back through the updater.

The owner's preference holds: the launcher is built inline, with one review at the end.

### Task 2 (subagent): hosting's behaviour (#359, the server and the console)
- **The terminal:**
  - the full-screen console no longer asks where the cursor is as it starts (`terminal.clear()` becomes a plain clear-screen), so a terminal that never answers costs nothing;
  - a console whose terminal has hung up ends, even when SIGHUP is ignored;
  - a console that `run` leaves behind can't leave the terminal raw;
  - an attached console exits 0 on a hang-up.
- **Panics:** a panic on a thread other than the console's goes to the log through the hub, cleaned, and leaves the console running. Final review B's shape: mark the console's thread, guard against re-entry, and keep the chain to the previous hook.
- **Config and ops:**
  - under `tls = "self"`, a start warning when `trusted_proxies` is set;
  - `ops` marks `[access].admins` names that have no account yet.
- **The cross-version rule.** An admin command that opens the store while a server holds `server.lock` never migrates the database. If the database's schema differs from the binary's, it refuses, pointing at `fenji-server console --run` or stopping the server first.

### Task 3 (subagent): packaging and the CLI launcher's words (#359)
- The Linux archives carry `deploy/systemd` and `deploy/caddy`, under the same `SHA256SUMS`, and the write-up uses them.
- `fenji-launch` says a spent or unknown invite in words, not "invalid_invite (HTTP 400)".
- A space typed inside a host name is refused, not joined (#358's rule, the `https://play example.net` case).

### Task 4 (subagent): the world's robustness (#281, Robustness)
- A hung tick is noticed. If the tick count makes no progress for about 30 s, the world logs an error and the server stops gracefully.
- The account check never stalls a player's connection: no store await inside the link's `select!`.
- A client-side network stall no longer gets a player kicked for flooding: a per-connection bucket refilled each tick, capped at `MAX_STALL`'s worth.
- `max_players` is capped below what one view's outbound queue holds, or the limit is documented.

### Task 5 (subagent): the world's tests and docs (#281, Tests, Docs batch, Known M4 limitations)
As #281 lists them, except the watchdog items, which are Task 6's.

### Task 6 (subagent): the runners and the watchdog (#244, #174, #281's watchdog item)
#244's items, #174's last two (`run-client.sh`'s guard against a prefix primed by another GE-Proton build, and hardening S3's ChangeServiceConfig rule), and #281's watchdog item, as one hardening pass. Repository-only, tested with the existing fakes.

### Task 7 (subagent): hosting's minors (#359, the tests and refactors)
#359's remaining minors:
- Tasks 4 and 6–11's doc comments, the view's scroll on resize, and `account_id`;
- the log hub's tests;
- Task 8's `check_text` and the alias equivalence test;
- Tasks 10 and 11's test gaps and `debug_assert`;
- thread-local log capture;
- cargo-deny's unused BSD-2-Clause allowance, as its own `chore(deps)`.

### Task 8 (local, RE and client runs): back to character select (#282)
Find what the client expects after type 4 in the world, answer it, and return the connection to the lobby. Then run world → character select → world in the sandbox.

### Task 9 (local, RE and client runs): #281's local checks
- The creation data other players receive: whether the name rule can spell the `locale_str` setter's unpickle prefix, and validating the face string as the client does.
- `!tp` across scenes: verify the client path in a run, then lift the refusal.

## Review Focus
1. **Nothing a player or a host does in the acceptance session should get worse.** A fix that changes a flow a host or player sees has its write-up line updated in the same PR.
2. **The launcher's one game at a time** holds across restarts and crashes, without ever refusing a game that has ended.
3. **The world's new limits** (the stall bucket, the tick watchdog) never punish an honest player on a slow link.
4. **The cross-version rule** never lets two binaries' migrations meet on a live database.
