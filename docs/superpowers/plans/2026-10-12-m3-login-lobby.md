# M3: Login and lobby — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. For M3 the owner chose inline execution (superpowers:executing-plans) by the controller, test-first, with a fresh reviewer subagent on each PR before it is squash-merged into `dev`.

**Goal:** The real, unmodified client logs in to Fenji's own server with a single-use ticket that the launcher's code fetched, lists, creates and deletes characters, and finds them again after a server restart, all inside the sandbox.

**Architecture:** Two pure libraries, `fenji-auth` (hashing, tokens, invites, rules, the sign-in limiter) and `fenji-store` (SQLite through `sqlx`). Over them, a `fenji-server` binary with an `axum` account API and the gate listener: it runs M2's `fenji_proto::handshake::Server`, checks the ticket, and answers the lobby's script messages (typed in a new `fenji_proto::lobby`). The `fenji-launcher` library and its `fenji-launch` CLI fetch the ticket and build the client's launch environment; the runner starts the client with it in the sandbox. `apps/launcher` is the Tauri shell over the same library.

**Tech Stack:** Rust 1.97.1 (edition 2024), tokio 1.53, sqlx 0.9 (SQLite, bundled), argon2 0.6, sha2 0.11, getrandom 0.4, axum 0.8, reqwest 0.13 (rustls + ring), serde/serde_json/toml 1.1, clap 4.6, Tauri 2.12 with Vite + TypeScript (pnpm 12), bash and Python stdlib for the runner.

**Spec:** [`docs/superpowers/specs/2026-10-12-m3-login-lobby-design.md`](../specs/2026-10-12-m3-login-lobby-design.md) (decisions D1–D20). Ledger: `.superpowers/sdd/2026-10-12-m3-login-lobby/progress.md` (git-ignored: rulings, run log, review log).

## Global Constraints

- **Ground rule 3: no game content in git.** Stub and server logs, captures, decoded archive files, table dumps (`re/m3/tables/`), decompiled or disassembled scripts and run evidence stay in the git-ignored `re/`. Never bypass `scripts/check-forbidden-files.sh`. The one client-derived file allowed in the repo is the launcher's manifest: names, sizes and SHA-256 hashes of the pinned build's files (D15).
- **Fixtures from client runs are minimal and scrubbed.** Lobby messages carry no machine data. Auth messages get M2's scrub: the 6 machine bytes become `02 00 00 00 00 01` and the 20-byte fingerprint becomes zeros, and a test checks both.
- **Clean room.** Script and Ghidra reading, REA and every client run are done only by a session or agent that has never read material rejected in `docs/re/sources.md`.
  - `docs/` holds prose and tables only, with no decompiled code. Identifiers, ids, constants, field names, error literals and file names are fine.
  - Tag findings **Observed**, **Inferred** or **Unknown**. Cite scripts by engine path and function path (`re/m1/disasm`), and native code by address and REA Evidence ID (`docs/re/README.md`, "The norm").
- **Ground rule 6 gates stop work:** client patching (on disk or in memory), circumventing protection, anything that would load the AGT driver, and Oodle outside the approved scope.
  - The server never sends script messages 243 (hotpatch files), 216 (GM scripts) or 556 (Lua bytecode).
  - Players on `gj_uinfo` are gate #114. The `Uinfo` login path compiles only under the cargo feature `dev-login-path`.
- **Ground rule 4: client runs.** Only through `scripts/spike/run-client.sh` (`--no-root --x11 99 --cpu-topology`, watchdog armed), on display `:99`.
  - If `/tmp/.X11-unix/X99` is missing, start `Xwayland :99 -geometry 1280x720 -ac -nolisten tcp -noreset -extension MIT-SHM -shm` in the background.
  - Never start Steam. Write each run's prediction in `re/m3/runs.md` before it starts.
  - Runner exit 3 is a gate event: stop ALL client runs and write it up.
- **Ground rule 7 and secrets.** Fenji never handles Steam or Gameforge credentials, and stores its passwords only as argon2id PHC strings.
  - Session tokens, game tokens and invite codes are stored only as SHA-256.
  - Fenji's code (server, stub, runner, launcher) never logs or prints a password, a session token, a game token, an invite code or the auth payload's ticket field, whatever it holds. Tests use synthetic values.
  - The auth payload's MAC address and fingerprint are dropped, never stored.
- **Rust conventions:**
  - Rust 1.97.1, edition 2024, `version.workspace = true`, `publish = false`, `license.workspace = true`.
  - `unsafe_code = "forbid"`, except `tools/oodle-helper`'s `dll.rs`.
  - Dependencies are declared once in the root `[workspace.dependencies]`.
  - `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` must pass.
  - Internal dependencies: `fenji-proto`, `fenji-lua`, `fenji-auth` and `fenji-store` depend on nothing internal. `fenji-server` depends on proto, auth, store and lua; `fenji-launcher` on auth.
- **`fenji-proto`:** no I/O and no tokio, and nothing allocated at a size a peer chose before a bound check. Malformed input returns `fenji_proto::Error`, and nothing panics on peer bytes. Every wire structure's doc comment cites its evidence.
- **Licences (D20):**
  - The root `deny.toml` keeps its permissive list and gains `[graph] targets = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc", "x86_64-pc-windows-gnu", "aarch64-apple-darwin"]`.
  - No `dirs` or `directories` crate in the root workspace.
  - `reqwest` uses `rustls-no-provider`, plus `rustls` with `ring`.
  - `apps/launcher` is its own Cargo workspace, whose `deny.toml` allows exactly `option-ext`, `cssparser`, `cssparser-macros`, `selectors` and `dtoa-short` under MPL-2.0.
- **Pinned build:** Steam build 10015727; `bin64/gujianol.exe` SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`; 210 files, 107,996,004,001 bytes.
- **Process:**
  - Conventional Commits, with scopes `proto`, `auth`, `store`, `server`, `launcher`, `tools`, `docs`, `re`, `deps` and `release`.
  - One issue, one branch (`<type>/<scope>-<issue>-<topic>`), one PR: `Closes #N`, the body per `.github/pull_request_template.md`, ending with the Claude Code line, with NO session links or IDs anywhere.
  - A fresh reviewer subagent (opus) reviews each PR clean before it is squash-merged into `dev`.
  - Stage by explicit paths, never `git add -A`, and never stage `.claude/settings.json`.
  - Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` only.

## Review Focus

1. **A connection that never authenticates.** A client that connects and sends nothing, or keeps sending bytes without completing its auth message, must be closed after the 30 s deadline or at 26 + 0x4000 bytes. It must not hold a task or memory forever (Task 6: `silent_connection_is_closed_after_the_deadline`, `oversized_pre_auth_input_is_refused`).
2. **A hostile or broken create request.** A create request that no client sends, such as a table where a scalar belongs, a face string over 4 KiB, or a missing name, vocation or skeleton, is refused with a 270 error. Nothing is stored, and the connection and server stay up (Task 4: `shapes_no_client_sends_are_refused`; Task 6: `oversized_appearance_is_refused_and_server_survives`).
3. **Two players racing for one name.** Two accounts creating the same name at once: exactly one gets the character, the other `ERR_ROLE_NAME_EXIST` (Task 3: `concurrent_creates_with_one_name_store_one`).
4. **The admin's CLI while the server runs.** `fenji-server invite create` against the database a running server holds open succeeds, through WAL and the busy timeout (Task 5: `cli_writes_while_a_server_holds_the_database`).
5. **A launcher pointed at the wrong place.** An `http://` address that isn't loopback, or a server name with no IPv4 address, is refused with a clear message before any password is sent (Task 7: `refuses_plain_http_off_loopback`, `gate_without_ipv4_is_a_clear_error`).

---

## File structure

| Path | Responsibility | Task |
|---|---|---|
| `docs/protocol/packets/lobby.md` | The lobby's messages, the actor table, errors and close reasons, with evidence | 1, 9 |
| `crates/fenji-auth/src/{lib,error,password,token,invite,rules,limiter}.rs` | Pure account primitives | 2 |
| `crates/fenji-store/migrations/0001_accounts_and_characters.sql`, `src/{lib,error,accounts,invites,tokens,characters}.rs` | SQLite persistence | 3 |
| `crates/fenji-proto/src/lobby.rs`, `src/pickle.rs` (`encode_compressed`, `resolve`), `src/client.rs` | Typed lobby messages, value validation, the simulator's fixes | 4 |
| `crates/fenji-server/src/{main,lib,config,clock,accounts,api,cli}.rs`, `fenji.example.toml` | Config, the account flows and API, admin commands | 5 |
| `crates/fenji-server/src/{server,gate,online,lobby}.rs`, `tests/{common/mod.rs,lobby.rs}` | The gate, limits, ticket check, lobby; the bot | 6 |
| `crates/fenji-launcher/src/{lib,api,manifest,verify,launch,login_path,invite_link,settings}.rs`, `src/bin/fenji-launch.rs`, `manifests/10015727.tsv` | Launcher library and CLI | 7 |
| `scripts/spike/{run-client.sh,xkey.py,test-run-client.sh}` | Runner: server mode, launcher hook, keys | 8 |
| `re/m3/runs.md` (local), fixtures under `crates/fenji-server/tests/fixtures/` | The rungs | 9 |
| `tools/stub/src/{main,speak,session}.rs` | `--no-auth-capture`, `--work-pad` | 10 |
| `apps/launcher/**`, `.github/workflows/ci.yml` | Tauri shell and its CI job | 11 |
| `docs/re/m3-summary.md`, parent spec, `login.md`, `opcodes.md`, README, PRIVACY, SECURITY, CLAUDE.md | Records | 12 |

Task N's issue is created when the plan PR opens; the ledger maps tasks to issue numbers. Execution order: 2, 3 and 10 can run before 1 lands; then 1, 4, 5, 6, 7, 8, 9, 11, 12, 13. A task's branch may stack on its parent's while the parent is in review, and is rebased onto `dev` once the parent lands.

---

### Task 1: Lobby and platform reading (W1)

**Files:**
- Create: `docs/protocol/packets/lobby.md`
- Modify: `docs/protocol/opcodes.md` (Script messages table), `docs/protocol/README.md` (Files table: `packets/lobby.md`), `docs/README.md` (index row)
- Local evidence (not committed): `re/m3/lobby-reading.md` (the reading agent's notes), `re/m3/platform-reading.md`, `re/m3/rea/`

**Interfaces:**
- Consumes: `re/m3/planning-findings.md`, `re/m3/tables/errors.jsonl`, `re/m1/decompiled-all/`, `re/m1/disasm/`.
- Produces: `docs/protocol/packets/lobby.md`, the evidence behind the values Tasks 4 and 6 already use (taken from the reading agent's notes, `re/m3/lobby-reading.md`, while this plan was written). If Step 1's checks change any of those values, the change goes into **Amendment A**, a short section appended to this plan in Task 1's PR. The values are:
  - the role-list entry's fields with the source of each (D8);
  - the 228, 628 and 84 argument values, and the 270 codes for each refusal;
  - the close-reason keys for auth failure, a full server and a duplicate login;
  - the name length rules, the number of character slots, and whether 402 needs a reply.

- [ ] **Step 1: Read the reading agent's notes and check them.** Read `re/m3/lobby-reading.md` whole. Pick its five load-bearing claims: the 228 argument order, the fields the select screen requires, the create request's types, the delete reply's values, and the error-code path. Check each against the decompiled script and its `.dis` function path. Record any disagreement in the ledger and resolve it from the script.

- [ ] **Step 2: Platform-path reading (S10).** Dispatch one fresh reading agent (opus), with the clean-room rules and these questions, to write `re/m3/platform-reading.md`:
  - (a) With `--gf-sdk=steam`, what `bin64/gameforge_api64.dll`'s Steam backend loads, starts and contacts, and whether it needs a running Steam client. Read it statically through REA on that DLL: one REA session, closed at the end, with Evidence IDs recorded.
  - (b) What `net.luac`'s Steam login branch sends to `gj_gates`: server id 99, zone `"80"`, the SteamID and Steam's ticket.
  - (c) Whether Fenji could bind a Steam ticket to an account without the publisher's key.
  - (d) What a stand-in `gameforge_client_api64_v2.dll` (gate #11's option b) would have to export, from the library's import of it.

  Wait for it, then check two of its claims in the scripts or through REA yourself.

- [ ] **Step 3: Write `docs/protocol/packets/lobby.md`** in the established form (method paragraph, tags, numbered findings, evidence index, REA Evidence IDs table). Sections:
  - **§1 Which copy loads:** the module loader's `data/locale/` rewrite, and `IsCommonLocale` and `GetServerLocale`.
  - **§2 Messages:** a table per direction (id, name where the client names it, arguments, handler, effect). It covers 292, 370, 496, 402, 304, 166, 10, 26, 454 and 321, and 228, 628, 84, 270, 414, 495, 509, 368, 786, 243, 216 and 556.
  - **§3 Title screen to list:** "Press Any Key", `QueryActors`, the transition, and creation vs select.
  - **§4 The actor table:** one row per field (type, use, required or tolerated, source per D8).
  - **§5 The create request:** fields and types, the client's own checks, the name rules.
  - **§6 The create result.**
  - **§7 Delete:** the typed answer, 166, and 84's three values.
  - **§8 Errors:** message 270, the `errors` sheet's `ID` and `literal`, the codes Fenji uses.
  - **§9 Close reasons:** the client's keys.
  - **§10 Entering the world:** 10, and the refusal M3 sends.
  - **§11 What Fenji sends:** the server's choices, message by message, with minimal values.
  - **§12 The platform paths:** Task 1 Step 2's findings.
  - **§13 Open questions for the runs.**

  Cite scripts as `data/<path>.luac` with their function path, and the `errors` rows by `literal` and `ID`.
- [ ] **Step 4: Update `opcodes.md`'s Script messages table** with every id in §2 (status `schema` or `named`; Task 9 raises the verified ones), and add `packets/lobby.md` to `docs/protocol/README.md` and `docs/README.md`.
- [ ] **Step 5: Amend only if needed.** If Step 1 changed a value that Tasks 4 or 6 use, append **Amendment A** at the end of this plan: the old and new value, and the `lobby.md` finding behind it. Otherwise write "no amendment" in the ledger.
- [ ] **Step 6: Comment on gate #114** with §12's findings: the Steam path's requirements, what it sends, and whether binding is possible.
- [ ] **Step 7: Check and commit.**

```bash
scripts/check-forbidden-files.sh
git add docs/protocol/packets/lobby.md docs/protocol/opcodes.md docs/protocol/README.md docs/README.md docs/superpowers/plans/2026-10-12-m3-login-lobby.md
git commit -m "docs(re): read the client's lobby and platform paths" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Then push, open the PR (`Closes #<Task 1 issue>`), have a fresh reviewer check the findings against the scripts, fix, and squash-merge once CI is green.

---

### Task 2: `fenji-auth` (W4)

**Files:**
- Create: `crates/fenji-auth/Cargo.toml`, `crates/fenji-auth/src/lib.rs`, `src/error.rs`, `src/password.rs`, `src/token.rs`, `src/invite.rs`, `src/rules.rs`, `src/limiter.rs`
- Modify: `Cargo.toml` (members, `[workspace.dependencies]`: `argon2 = "0.6.0"`, `getrandom = "0.4.3"`, `fenji-auth = { path = "crates/fenji-auth" }`; `[profile.dev.package.argon2] opt-level = 3` and the same for `blake2`), `Cargo.lock`

**Interfaces:**
- Produces:
  - `fenji_auth::password::{hash(&str) -> Result<String>, verify(&str, &str) -> Result<bool>, verify_dummy(&str)}`
  - `fenji_auth::token::{Token, PREFIX}`, with `Token::generate() -> Result<Token>`, `Token::parse(&str) -> Option<Token>`, `Token::as_string(&self) -> String` and `Token::hash(&self) -> [u8; 32]`
  - `fenji_auth::invite::InviteCode`, with `generate() -> Result<Self>`, `parse(&str) -> Option<Self>`, `display(&self) -> String` and `hash(&self) -> [u8; 32]`
  - `fenji_auth::rules::{check_username(&str) -> Result<(), RuleError>, check_password(&str) -> Result<(), RuleError>, RuleError}`
  - `fenji_auth::limiter::{Limiter, LimiterConfig, Verdict}`
  - `fenji_auth::{Error, Result}`

- [ ] **Step 1: Scaffold the crate.**

`crates/fenji-auth/Cargo.toml`:
```toml
[package]
name = "fenji-auth"
description = "Fenji's own accounts without storage: argon2id password hashes, tokens and invite codes, the username and password rules, and the sign-in limiter"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true
license.workspace = true

[dependencies]
argon2.workspace = true
getrandom.workspace = true
sha2.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

`src/lib.rs`:
```rust
//! Fenji's own accounts, without storage (M3 spec D6): password hashes, tokens and invite codes,
//! the rules for usernames and passwords, and the sign-in limiter. `fenji-server` runs the flows
//! against `fenji-store`; nothing here does I/O apart from reading the OS's random source.
mod error;
pub mod invite;
pub mod limiter;
pub mod password;
pub mod rules;
pub mod token;

pub use error::{Error, Result};
```

`src/error.rs`:
```rust
/// What can go wrong here. None of the messages carries a secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("the OS random source failed")]
    Random,
    #[error("password hashing failed: {0}")]
    Hash(String),
    #[error("a stored password hash can't be read")]
    BadStoredHash,
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
```

Add `"crates/fenji-auth"` to the workspace members, the dependencies above, and the two profile overrides (argon2 in an unoptimised test build takes seconds per hash):
```toml
[profile.dev.package.argon2]
opt-level = 3

[profile.dev.package.blake2]
opt-level = 3
```

- [ ] **Step 2: Write the failing tests for `password` and `rules`** (in each module's `#[cfg(test)] mod tests`):

```rust
// password.rs
#[test]
fn hash_is_argon2id_with_the_default_parameters_and_verifies() {
    let phc = hash("correct horse battery").unwrap();
    assert!(phc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{phc}");
    assert!(verify("correct horse battery", &phc).unwrap());
    assert!(!verify("correct horse batterY", &phc).unwrap());
}

#[test]
fn two_hashes_of_one_password_differ_by_salt() {
    assert_ne!(hash("same password!").unwrap(), hash("same password!").unwrap());
}

#[test]
fn a_damaged_stored_hash_is_an_error_not_a_mismatch() {
    assert_eq!(verify("x", "not a phc string"), Err(crate::Error::BadStoredHash));
}

#[test]
fn verify_dummy_runs() {
    verify_dummy("anything at all");
}

// rules.rs
#[test]
fn usernames() {
    assert_eq!(check_username("abc"), Ok(()));
    assert_eq!(check_username("Ab_c-9"), Ok(()));
    assert_eq!(check_username(&"a".repeat(16)), Ok(()));
    assert_eq!(check_username("ab"), Err(RuleError::UsernameLength));
    assert_eq!(check_username(&"a".repeat(17)), Err(RuleError::UsernameLength));
    assert_eq!(check_username("a:bc"), Err(RuleError::UsernameChars));
    assert_eq!(check_username("ab c"), Err(RuleError::UsernameChars));
    assert_eq!(check_username("abé"), Err(RuleError::UsernameChars));
}

#[test]
fn passwords_count_characters_not_bytes() {
    assert_eq!(check_password(&"é".repeat(10)), Ok(()));
    assert_eq!(check_password(&"a".repeat(9)), Err(RuleError::PasswordLength));
    assert_eq!(check_password(&"a".repeat(128)), Ok(()));
    assert_eq!(check_password(&"a".repeat(129)), Err(RuleError::PasswordLength));
}
```

- [ ] **Step 3: Run them and see them fail** (`cargo test -p fenji-auth`: unresolved names).

- [ ] **Step 4: Implement `password` and `rules`.**

`src/password.rs`:
```rust
//! Password hashes (ground rule 7): argon2id with the `argon2` crate's defaults, 19 MiB, 2 passes
//! and 1 lane (OWASP's recommendation), stored as PHC strings such as
//! `$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>`.
use std::sync::OnceLock;

use argon2::password_hash::Error as PhError;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use crate::{Error, Result};

/// Hashes `password` with a fresh salt.
pub fn hash(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| Error::Hash(e.to_string()))
}

/// Whether `password` matches the stored PHC string. A stored hash that doesn't parse is an error,
/// not a mismatch, so that a damaged row gets noticed.
pub fn verify(password: &str, phc: &str) -> Result<bool> {
    let parsed = PasswordHash::new(phc).map_err(|_| Error::BadStoredHash)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(PhError::PasswordInvalid) => Ok(false),
        Err(_) => Err(Error::BadStoredHash),
    }
}

/// Spends the time a real check takes, against a fixed hash, for a username that doesn't exist:
/// so the time taken doesn't tell an unknown username from a wrong password.
pub fn verify_dummy(password: &str) {
    static DUMMY: OnceLock<String> = OnceLock::new();
    let phc = DUMMY.get_or_init(|| hash("fenji: no such account").expect("hashing a constant"));
    let _ = verify(password, phc);
}
```

`src/rules.rs`:
```rust
//! The rules Fenji applies to new usernames and passwords (M3 spec §4).

/// Usernames: 3 to 16 characters from `A–Z a–z 0–9 _ -`. The client splits `gj_uinfo` on `:`
/// (docs/protocol/login.md 2.8), so a colon must never pass.
pub const USERNAME_LEN: std::ops::RangeInclusive<usize> = 3..=16;
/// Passwords: 10 to 128 characters (Unicode scalar values), nothing else required.
pub const PASSWORD_LEN: std::ops::RangeInclusive<usize> = 10..=128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleError {
    UsernameLength,
    UsernameChars,
    PasswordLength,
}

impl std::fmt::Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UsernameLength => "a username has 3 to 16 characters",
            Self::UsernameChars => "a username uses only A-Z, a-z, 0-9, _ and -",
            Self::PasswordLength => "a password has 10 to 128 characters",
        })
    }
}

pub fn check_username(name: &str) -> Result<(), RuleError> {
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(RuleError::UsernameChars);
    }
    if !USERNAME_LEN.contains(&name.len()) {
        return Err(RuleError::UsernameLength);
    }
    Ok(())
}

pub fn check_password(password: &str) -> Result<(), RuleError> {
    if PASSWORD_LEN.contains(&password.chars().count()) {
        Ok(())
    } else {
        Err(RuleError::PasswordLength)
    }
}
```
(The character check runs first, so `"abé"` reports its characters, and for ASCII the byte length is the character count.)

- [ ] **Step 5: Run `cargo test -p fenji-auth`; expect PASS.**

- [ ] **Step 6: Write the failing tests for `token` and `invite`.**

```rust
// token.rs
#[test]
fn a_token_round_trips_through_its_string() {
    let t = Token::generate().unwrap();
    let s = t.as_string();
    assert!(s.starts_with(PREFIX));
    assert_eq!(s.len(), PREFIX.len() + 64);
    assert_eq!(Token::parse(&s).unwrap(), t);
    assert_eq!(Token::parse(&s.to_uppercase().replacen("FENJI1-", PREFIX, 1)).unwrap(), t);
}

#[test]
fn parse_refuses_anything_else() {
    let s = Token::generate().unwrap().as_string();
    assert!(Token::parse(&s[PREFIX.len()..]).is_none(), "no prefix");
    assert!(Token::parse(&s[..s.len() - 1]).is_none(), "short");
    assert!(Token::parse(&format!("{s}0")).is_none(), "long");
    assert!(Token::parse(&format!("{PREFIX}{}", "g".repeat(64))).is_none(), "not hex");
    assert!(Token::parse("").is_none());
}

#[test]
fn tokens_differ_and_hash_to_sha256_of_their_bytes() {
    let a = Token::generate().unwrap();
    let b = Token::generate().unwrap();
    assert_ne!(a, b);
    assert_ne!(a.hash(), b.hash());
    assert_eq!(a.hash(), a.clone().hash());
}

#[test]
fn debug_never_shows_the_token() {
    let t = Token::generate().unwrap();
    let hex = &t.as_string()[PREFIX.len()..];
    assert!(!format!("{t:?}").contains(&hex[..8]));
}

// invite.rs
#[test]
fn a_code_displays_in_four_groups_and_parses_back() {
    let c = InviteCode::generate().unwrap();
    let d = c.display();
    assert_eq!(d.len(), 19);
    assert_eq!(d.matches('-').count(), 3);
    assert!(d.chars().all(|ch| ch == '-' || ALPHABET.contains(&(ch as u8))));
    assert_eq!(InviteCode::parse(&d).unwrap(), c);
}

#[test]
fn parse_normalises_what_people_type() {
    let c = InviteCode::parse("0123-4567-89AB-CDEF").unwrap();
    assert_eq!(InviteCode::parse("0123456789abcdef").unwrap(), c);
    assert_eq!(InviteCode::parse(" o123 4567 89ab cdef ").unwrap(), c); // O reads as 0
    let one = InviteCode::parse("1111-1111-1111-1111").unwrap();
    assert_eq!(InviteCode::parse("iIlL-1111-1111-1111").unwrap(), one); // I and L read as 1
    assert!(InviteCode::parse("UUUU-1111-1111-1111").is_none()); // U isn't in the alphabet
    assert!(InviteCode::parse("0123-4567-89AB-CDE").is_none());
    assert!(InviteCode::parse("0123-4567-89AB-CDEF0").is_none());
}

#[test]
fn the_hash_is_of_the_normalised_code() {
    let a = InviteCode::parse("0123-4567-89AB-CDEF").unwrap();
    let b = InviteCode::parse("0123456789abcdef").unwrap();
    assert_eq!(a.hash(), b.hash());
    assert!(!format!("{a:?}").contains("0123"));
}
```
- [ ] **Step 7: Run; expect FAIL. Implement.**

`src/token.rs`:
```rust
//! Session and game tokens (M3 spec D3): 32 bytes from the OS's random source, written as
//! `fenji1-` and 64 hex digits, and stored only as the SHA-256 of the 32 bytes. The prefix keeps
//! a ticket from ever matching the `^%x+$` branch of the client's `net_manager_login`, which
//! would hex-decode it (docs/protocol/packets/lobby.md).
use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Every token's prefix.
pub const PREFIX: &str = "fenji1-";

#[derive(Clone, PartialEq, Eq)]
pub struct Token([u8; 32]);

impl Token {
    pub fn generate() -> Result<Self> {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).map_err(|_| Error::Random)?;
        Ok(Self(b))
    }

    /// `fenji1-` and 64 hex digits, either case; anything else is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.get(..PREFIX.len())?.eq_ignore_ascii_case(PREFIX).then(|| &s[PREFIX.len()..])?;
        if hex.len() != 64 {
            return None;
        }
        let mut b = [0u8; 32];
        for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
            b[i] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Some(Self(b))
    }

    pub fn as_string(&self) -> String {
        let mut s = String::with_capacity(PREFIX.len() + 64);
        s.push_str(PREFIX);
        for byte in self.0 {
            s.push_str(&format!("{byte:02x}"));
        }
        s
    }

    /// What the store keeps.
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.0).into()
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}
```

`src/invite.rs`:
```rust
//! Invite codes (M3 spec D13): 80 random bits in Crockford's base32, 16 symbols shown in four
//! groups (`ABCD-EFGH-JKMN-PQRS`), stored only as the SHA-256 of the 16 normalised symbols.
use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Crockford's base32 alphabet: no I, L, O or U.
pub const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Clone, PartialEq, Eq)]
pub struct InviteCode([u8; 16]);

impl InviteCode {
    pub fn generate() -> Result<Self> {
        let mut raw = [0u8; 10];
        getrandom::fill(&mut raw).map_err(|_| Error::Random)?;
        let bits = raw.iter().fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
        let mut out = [0u8; 16];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = ALPHABET[((bits >> (75 - 5 * i)) & 31) as usize];
        }
        Ok(Self(out))
    }

    /// Accepts either case, dashes and spaces anywhere; reads O as 0 and I or L as 1.
    pub fn parse(input: &str) -> Option<Self> {
        let mut out = [0u8; 16];
        let mut n = 0;
        for c in input.bytes() {
            let c = match c.to_ascii_uppercase() {
                b'-' | b' ' => continue,
                b'O' => b'0',
                b'I' | b'L' => b'1',
                c if ALPHABET.contains(&c) => c,
                _ => return None,
            };
            *out.get_mut(n)? = c;
            n += 1;
        }
        (n == 16).then_some(Self(out))
    }

    pub fn display(&self) -> String {
        let s = std::str::from_utf8(&self.0).expect("ASCII alphabet");
        format!("{}-{}-{}-{}", &s[..4], &s[4..8], &s[8..12], &s[12..])
    }

    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.0).into()
    }
}

impl std::fmt::Debug for InviteCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InviteCode(…)")
    }
}
```

- [ ] **Step 8: Run; expect PASS.**

- [ ] **Step 9: Write the failing tests for `limiter`.**

```rust
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

fn ip(n: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(10, 0, 0, n))
}

fn limiter() -> Limiter {
    Limiter::new(LimiterConfig::default())
}

#[test]
fn five_failures_lock_the_pair_but_not_another_address() {
    let mut l = limiter();
    let t = Instant::now();
    for _ in 0..5 {
        assert_eq!(l.check("alice", ip(1), t), Verdict::Allow);
        l.record_failure("alice", ip(1), t);
    }
    assert!(matches!(l.check("alice", ip(1), t), Verdict::Locked { .. }));
    assert_eq!(l.check("alice", ip(2), t), Verdict::Allow);
}

#[test]
fn the_lock_ends_and_success_clears_the_pair() {
    let mut l = limiter();
    let t = Instant::now();
    for _ in 0..5 {
        l.record_failure("bob", ip(1), t);
    }
    let later = t + Duration::from_secs(15 * 60 + 1);
    assert_eq!(l.check("bob", ip(1), later), Verdict::Allow);
    for _ in 0..4 {
        l.record_failure("bob", ip(1), later);
    }
    l.record_success("bob", ip(1));
    l.record_failure("bob", ip(1), later);
    assert_eq!(l.check("bob", ip(1), later), Verdict::Allow);
}

#[test]
fn twenty_failures_from_one_address_lock_the_address() {
    let mut l = limiter();
    let t = Instant::now();
    for i in 0..20 {
        l.record_failure(&format!("user{i}"), ip(9), t);
    }
    assert!(matches!(l.check("someone-else", ip(9), t), Verdict::Locked { .. }));
    assert_eq!(l.check("someone-else", ip(8), t), Verdict::Allow);
}

#[test]
fn many_failures_for_one_username_only_slow_it_down() {
    let mut l = limiter();
    let t = Instant::now();
    for i in 0..10 {
        l.record_failure("carol", ip(i), t);
    }
    // From a fresh address: never locked, but spaced 2 s apart.
    assert!(matches!(l.check("carol", ip(100), t), Verdict::SlowDown { .. }));
    assert_eq!(l.check("carol", ip(100), t + Duration::from_secs(2)), Verdict::Allow);
}

#[test]
fn usernames_compare_case_insensitively() {
    let mut l = limiter();
    let t = Instant::now();
    for _ in 0..5 {
        l.record_failure("Dave", ip(1), t);
    }
    assert!(matches!(l.check("dave", ip(1), t), Verdict::Locked { .. }));
}

#[test]
fn the_key_count_stays_bounded() {
    let mut l = Limiter::new(LimiterConfig { max_keys: 100, ..LimiterConfig::default() });
    let t = Instant::now();
    for i in 0..10_000u32 {
        l.record_failure(&format!("u{i}"), IpAddr::V4(Ipv4Addr::from(i)), t);
    }
    assert!(l.key_count() <= 3 * 100);
}
```

- [ ] **Step 10: Run; expect FAIL. Implement `src/limiter.rs`.**

```rust
//! The sign-in limiter (M3 spec D12). Failures are counted in a sliding window per (username,
//! address) pair and per address, each of which locks for a while, and per username, which only
//! slows down: one attempt every `slow_interval`. So nobody can lock another player out by
//! knowing their username. In memory: a restart forgets it, which suits a small server.
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct LimiterConfig {
    pub pair_max: usize,
    pub addr_max: usize,
    pub user_max: usize,
    pub window: Duration,
    pub lock: Duration,
    pub slow_interval: Duration,
    /// Keys kept per kind before old ones are dropped.
    pub max_keys: usize,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            pair_max: 5,
            addr_max: 20,
            user_max: 10,
            window: Duration::from_secs(15 * 60),
            lock: Duration::from_secs(15 * 60),
            slow_interval: Duration::from_secs(2),
            max_keys: 10_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Locked { retry_after: Duration },
    SlowDown { retry_after: Duration },
}

#[derive(Default)]
struct Entry {
    failures: VecDeque<Instant>,
    locked_until: Option<Instant>,
    last_attempt: Option<Instant>,
}

impl Entry {
    fn trim(&mut self, now: Instant, window: Duration) {
        while self.failures.front().is_some_and(|t| now.duration_since(*t) >= window) {
            self.failures.pop_front();
        }
    }

    fn stale(&self, now: Instant, window: Duration) -> bool {
        self.failures.back().is_none_or(|t| now.duration_since(*t) >= window)
            && self.locked_until.is_none_or(|u| u <= now)
    }
}

pub struct Limiter {
    cfg: LimiterConfig,
    pairs: HashMap<(String, IpAddr), Entry>,
    addrs: HashMap<IpAddr, Entry>,
    users: HashMap<String, Entry>,
}

impl Limiter {
    pub fn new(cfg: LimiterConfig) -> Self {
        Self { cfg, pairs: HashMap::new(), addrs: HashMap::new(), users: HashMap::new() }
    }

    /// Whether an attempt may go ahead now. A `SlowDown` username's attempt is noted as made.
    pub fn check(&mut self, username: &str, addr: IpAddr, now: Instant) -> Verdict {
        let user = username.to_ascii_lowercase();
        for until in [
            self.pairs.get(&(user.clone(), addr)).and_then(|e| e.locked_until),
            self.addrs.get(&addr).and_then(|e| e.locked_until),
        ]
        .into_iter()
        .flatten()
        {
            if until > now {
                return Verdict::Locked { retry_after: until - now };
            }
        }
        let (window, max, interval) = (self.cfg.window, self.cfg.user_max, self.cfg.slow_interval);
        if let Some(e) = self.users.get_mut(&user) {
            e.trim(now, window);
            if e.failures.len() >= max {
                if let Some(last) = e.last_attempt {
                    let since = now.duration_since(last);
                    if since < interval {
                        return Verdict::SlowDown { retry_after: interval - since };
                    }
                }
                e.last_attempt = Some(now);
            }
        }
        Verdict::Allow
    }

    pub fn record_failure(&mut self, username: &str, addr: IpAddr, now: Instant) {
        let user = username.to_ascii_lowercase();
        let cfg = self.cfg.clone();
        let pair = self.pairs.entry((user.clone(), addr)).or_default();
        bump(pair, now, &cfg, cfg.pair_max, true);
        let a = self.addrs.entry(addr).or_default();
        bump(a, now, &cfg, cfg.addr_max, true);
        let u = self.users.entry(user).or_default();
        bump(u, now, &cfg, cfg.user_max, false);
        u.last_attempt = Some(now);
        self.prune(now);
    }

    pub fn record_success(&mut self, username: &str, addr: IpAddr) {
        self.pairs.remove(&(username.to_ascii_lowercase(), addr));
    }

    /// For tests: keys held across the three maps.
    pub fn key_count(&self) -> usize {
        self.pairs.len() + self.addrs.len() + self.users.len()
    }

    fn prune(&mut self, now: Instant) {
        let (window, max) = (self.cfg.window, self.cfg.max_keys);
        prune_map(&mut self.pairs, now, window, max);
        prune_map(&mut self.addrs, now, window, max);
        prune_map(&mut self.users, now, window, max);
    }
}

fn bump(e: &mut Entry, now: Instant, cfg: &LimiterConfig, max: usize, locks: bool) {
    e.trim(now, cfg.window);
    e.failures.push_back(now);
    if locks && e.failures.len() >= max {
        e.locked_until = Some(now + cfg.lock);
        e.failures.clear();
    }
}

fn prune_map<K: Clone + Eq + std::hash::Hash>(map: &mut HashMap<K, Entry>, now: Instant, window: Duration, max: usize) {
    if map.len() <= max {
        return;
    }
    map.retain(|_, e| !e.stale(now, window));
    if map.len() > max {
        // Under a flood of fresh keys: keep the newest `max`, so memory stays bounded.
        let mut keys: Vec<(K, Instant)> = map
            .iter()
            .map(|(k, e)| (k.clone(), e.failures.back().copied().unwrap_or(now)))
            .collect();
        keys.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
        for (k, _) in keys.into_iter().skip(max) {
            map.remove(&k);
        }
    }
}
```

- [ ] **Step 11: Run `cargo test -p fenji-auth`, then the full gate.**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo deny check
```
Expected: all pass. Fix any lint by changing the code, not by allowing the lint.

- [ ] **Step 12: Commit, push, PR, review, merge.**

```bash
git add Cargo.toml Cargo.lock crates/fenji-auth
git commit -m "feat(auth): add fenji-auth: password hashes, tokens, invite codes, rules and the limiter" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `fenji-store` (W3)

**Files:**
- Create: `crates/fenji-store/Cargo.toml`, `crates/fenji-store/migrations/0001_accounts_and_characters.sql`, `src/lib.rs`, `src/error.rs`, `src/accounts.rs`, `src/invites.rs`, `src/tokens.rs`, `src/characters.rs`
- Modify: `Cargo.toml` (member; `sqlx = { version = "0.9.0", default-features = false, features = ["runtime-tokio", "sqlite-bundled", "migrate", "macros", "derive"] }`; `fenji-store = { path = "crates/fenji-store" }`), `Cargo.lock`

**Interfaces:**
- Produces (all `async`, on `fenji_store::Store`; times are Unix ms; hashes are `&[u8; 32]`):
  - `Store::open(&Path) -> Result<Store>` and `close(&self)`.
  - Invites:
    - `create_invite(code_hash, label: Option<&str>, now, expires_at: Option<i64>) -> Result<i64>` returns the invite id.
    - `list_invites() -> Result<Vec<InviteRow>>`.
    - `revoke_invite(id: i64, now) -> Result<bool>`.
    - `redeem_invite(code_hash, username, password_phc, now) -> Result<i64>` returns the account id, or `Error::InviteInvalid` or `Error::UsernameTaken`.
  - Accounts:
    - `account_by_username(&str) -> Result<Option<Account>>` and `account_by_id(i64) -> Result<Option<Account>>`.
    - `record_sign_in(id, now) -> Result<()>`.
    - `list_accounts() -> Result<Vec<AccountSummary>>`.
    - `delete_account(&str) -> Result<bool>`.
  - Sessions and game tokens:
    - `create_session(token_hash, account_id, now, expires_at) -> Result<()>`, `session_account(token_hash, now) -> Result<Option<i64>>` and `delete_session(token_hash) -> Result<bool>`.
    - `create_game_token(token_hash, account_id, expires_at) -> Result<()>` and `consume_game_token(token_hash, now) -> Result<Option<i64>>`.
    - `purge_expired(now) -> Result<u64>`.
  - Characters:
    - `list_characters(account_id) -> Result<Vec<Character>>`.
    - `create_character(account_id, &NewCharacter, max_per_account: u32, now) -> Result<Character>`, which can fail with `Error::NameTaken` or `Error::TooMany`.
    - `delete_character(account_id, pid) -> Result<bool>`.
  - Types:
    - `Account { id, username, password_phc, created_at, last_sign_in_at: Option<i64> }`
    - `AccountSummary { id, username, created_at, last_sign_in_at, characters: i64 }`
    - `InviteRow { id, label: Option<String>, created_at, expires_at: Option<i64>, revoked_at: Option<i64>, redeemed_by: Option<String>, redeemed_at: Option<i64> }`
    - `NewCharacter { name: String, vocation: i64, skeleton: i64, appearance: Vec<u8> }`
    - `Character { pid, account_id, name, vocation, skeleton, level, appearance: Vec<u8>, created_at }`
    - `Error { Db(sqlx::Error), Migrate(String), InviteInvalid, UsernameTaken, NameTaken, TooMany }`

- [ ] **Step 1: Scaffold.** `Cargo.toml` as `fenji-auth`'s, with description "Fenji's SQLite store: accounts, invites, sessions, game tokens and characters", dependencies `sqlx.workspace = true`, `thiserror.workspace = true`; dev-dependencies `tempfile.workspace = true`, `tokio.workspace = true`. The migration:

```sql
-- M3 (docs/superpowers/specs/2026-10-12-m3-login-lobby-design.md §4): times are Unix milliseconds;
-- invite codes, session tokens and game tokens are SHA-256 hashes, never the secrets.
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_phc TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_sign_in_at INTEGER
) STRICT;

CREATE TABLE invites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code_hash BLOB NOT NULL UNIQUE,
    label TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER,
    revoked_at INTEGER,
    redeemed_by INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    redeemed_at INTEGER
) STRICT;

CREATE TABLE sessions (
    token_hash BLOB PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE TABLE game_tokens (
    token_hash BLOB PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL,
    used_at INTEGER
) STRICT;

-- AUTOINCREMENT: a deleted character's pid is never handed out again.
CREATE TABLE characters (
    pid INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    vocation INTEGER NOT NULL,
    skeleton INTEGER NOT NULL,
    level INTEGER NOT NULL DEFAULT 1,
    appearance BLOB NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX characters_by_account ON characters(account_id);
CREATE INDEX sessions_by_account ON sessions(account_id);
CREATE INDEX game_tokens_by_account ON game_tokens(account_id);
```

`src/error.rs`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
    #[error("migration: {0}")]
    Migrate(String),
    #[error("the invite is unknown, used, revoked or expired")]
    InviteInvalid,
    #[error("that username is taken")]
    UsernameTaken,
    #[error("that character name is taken")]
    NameTaken,
    #[error("the account has as many characters as it may")]
    TooMany,
}

impl From<sqlx::migrate::MigrateError> for Error {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        Self::Migrate(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Whether `e` is SQLite's UNIQUE constraint failure.
pub(crate) fn is_unique(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.is_unique_violation())
}
```

`src/lib.rs`:
```rust
//! Fenji's SQLite store (M3 spec D7, §4): accounts, invites, sessions, game tokens and characters,
//! in one file in WAL mode with foreign keys on. Every write that must be atomic is one
//! `BEGIN IMMEDIATE` transaction, so two writers queue on the busy timeout instead of failing
//! halfway. Times are Unix milliseconds; secrets arrive here already hashed.
use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};

mod accounts;
mod characters;
mod error;
mod invites;
mod tokens;

pub use accounts::{Account, AccountSummary};
pub use characters::{Character, NewCharacter};
pub use error::{Error, Result};
pub use invites::InviteRow;

/// The database, shared by every task of the server (a cheap clone of the pool).
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Opens `path`, creating it if needed, and brings its schema up to date.
    pub async fn open(path: &Path) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn begin(&self) -> Result<Transaction<'static, Sqlite>> {
        Ok(self.pool.begin_with("BEGIN IMMEDIATE").await?)
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    pub async fn store() -> (tempfile::TempDir, super::Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = super::Store::open(&dir.path().join("fenji.db")).await.unwrap();
        (dir, s)
    }

    pub const NOW: i64 = 1_800_000_000_000;

    pub fn h(n: u8) -> [u8; 32] {
        [n; 32]
    }
}
```

- [ ] **Step 2: Write the failing tests** (in the modules named; each `#[tokio::test]`, using `crate::test_util::{store, NOW, h}`):

```rust
// lib.rs tests
#[tokio::test]
async fn open_uses_wal_and_keeps_data_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fenji.db");
    let s = Store::open(&path).await.unwrap();
    let mode: (String,) = sqlx::query_as("PRAGMA journal_mode").fetch_one(&s.pool).await.unwrap();
    assert_eq!(mode.0, "wal");
    s.create_invite(&[7; 32], Some("for bob"), 1, None).await.unwrap();
    s.close().await;
    let s = Store::open(&path).await.unwrap();
    assert_eq!(s.list_invites().await.unwrap().len(), 1);
}

// invites.rs tests
#[tokio::test]
async fn an_invite_redeems_once() {
    let (_d, s) = store().await;
    s.create_invite(&h(1), Some("bob"), NOW, None).await.unwrap();
    let id = s.redeem_invite(&h(1), "bob", "$phc", NOW + 1).await.unwrap();
    assert!(matches!(s.redeem_invite(&h(1), "bob2", "$phc", NOW + 2).await, Err(Error::InviteInvalid)));
    let rows = s.list_invites().await.unwrap();
    assert_eq!(rows[0].redeemed_by.as_deref(), Some("bob"));
    assert_eq!(s.account_by_id(id).await.unwrap().unwrap().username, "bob");
}

#[tokio::test]
async fn unknown_revoked_and_expired_invites_are_invalid() {
    let (_d, s) = store().await;
    assert!(matches!(s.redeem_invite(&h(9), "x1x", "$p", NOW).await, Err(Error::InviteInvalid)));
    let id = s.create_invite(&h(2), None, NOW, None).await.unwrap();
    assert!(s.revoke_invite(id, NOW).await.unwrap());
    assert!(matches!(s.redeem_invite(&h(2), "x2x", "$p", NOW).await, Err(Error::InviteInvalid)));
    s.create_invite(&h(3), None, NOW, Some(NOW + 10)).await.unwrap();
    assert!(matches!(s.redeem_invite(&h(3), "x3x", "$p", NOW + 10).await, Err(Error::InviteInvalid)));
}

#[tokio::test]
async fn a_taken_username_leaves_the_invite_unused() {
    let (_d, s) = store().await;
    s.create_invite(&h(1), None, NOW, None).await.unwrap();
    s.create_invite(&h(2), None, NOW, None).await.unwrap();
    s.redeem_invite(&h(1), "Alice", "$p", NOW).await.unwrap();
    assert!(matches!(s.redeem_invite(&h(2), "alice", "$p", NOW).await, Err(Error::UsernameTaken)));
    s.redeem_invite(&h(2), "alice2", "$p", NOW).await.unwrap();
}

// tokens.rs tests
#[tokio::test]
async fn sessions_expire_and_end() {
    let (_d, s) = store().await;
    s.create_invite(&h(1), None, NOW, None).await.unwrap();
    let a = s.redeem_invite(&h(1), "bob", "$p", NOW).await.unwrap();
    s.create_session(&h(5), a, NOW, NOW + 100).await.unwrap();
    assert_eq!(s.session_account(&h(5), NOW + 99).await.unwrap(), Some(a));
    assert_eq!(s.session_account(&h(5), NOW + 100).await.unwrap(), None);
    assert!(s.delete_session(&h(5)).await.unwrap());
    assert_eq!(s.session_account(&h(5), NOW).await.unwrap(), None);
}

#[tokio::test]
async fn a_game_token_is_consumed_once_and_only_in_time() {
    let (_d, s) = store().await;
    s.create_invite(&h(1), None, NOW, None).await.unwrap();
    let a = s.redeem_invite(&h(1), "bob", "$p", NOW).await.unwrap();
    s.create_game_token(&h(6), a, NOW + 60_000).await.unwrap();
    assert_eq!(s.consume_game_token(&h(6), NOW + 1).await.unwrap(), Some(a));
    assert_eq!(s.consume_game_token(&h(6), NOW + 2).await.unwrap(), None);
    s.create_game_token(&h(7), a, NOW + 60_000).await.unwrap();
    assert_eq!(s.consume_game_token(&h(7), NOW + 60_000).await.unwrap(), None);
    assert_eq!(s.consume_game_token(&h(8), NOW).await.unwrap(), None);
}

#[tokio::test]
async fn purge_drops_expired_sessions_and_spent_tokens() {
    let (_d, s) = store().await;
    s.create_invite(&h(1), None, NOW, None).await.unwrap();
    let a = s.redeem_invite(&h(1), "bob", "$p", NOW).await.unwrap();
    s.create_session(&h(2), a, NOW, NOW + 10).await.unwrap();
    s.create_session(&h(3), a, NOW, NOW + 1_000).await.unwrap();
    s.create_game_token(&h(4), a, NOW + 10).await.unwrap();
    s.create_game_token(&h(5), a, NOW + 1_000).await.unwrap();
    s.consume_game_token(&h(5), NOW).await.unwrap();
    assert_eq!(s.purge_expired(NOW + 20).await.unwrap(), 3);
    assert_eq!(s.session_account(&h(3), NOW + 20).await.unwrap(), Some(a));
}

// characters.rs tests
fn new(name: &str) -> NewCharacter {
    NewCharacter { name: name.into(), vocation: 1, skeleton: 2, appearance: vec![1, 2, 3] }
}

async fn account(s: &Store, n: u8, name: &str) -> i64 {
    s.create_invite(&h(n), None, NOW, None).await.unwrap();
    s.redeem_invite(&h(n), name, "$p", NOW).await.unwrap()
}

#[tokio::test]
async fn create_list_and_delete() {
    let (_d, s) = store().await;
    let a = account(&s, 1, "bob").await;
    let c = s.create_character(a, &new("Lin"), 4, NOW).await.unwrap();
    assert_eq!((c.name.as_str(), c.level, c.appearance.as_slice()), ("Lin", 1, &[1u8, 2, 3][..]));
    assert_eq!(s.list_characters(a).await.unwrap(), vec![c.clone()]);
    let other = account(&s, 2, "eve").await;
    assert!(!s.delete_character(other, c.pid).await.unwrap(), "not eve's");
    assert!(s.delete_character(a, c.pid).await.unwrap());
    assert!(s.list_characters(a).await.unwrap().is_empty());
    let again = s.create_character(a, &new("Lin"), 4, NOW).await.unwrap();
    assert!(again.pid > c.pid, "a pid is never reused");
}

#[tokio::test]
async fn names_are_unique_case_insensitively_and_accounts_are_capped() {
    let (_d, s) = store().await;
    let a = account(&s, 1, "bob").await;
    let b = account(&s, 2, "eve").await;
    s.create_character(a, &new("Lin"), 2, NOW).await.unwrap();
    assert!(matches!(s.create_character(b, &new("LIN"), 2, NOW).await, Err(Error::NameTaken)));
    s.create_character(a, &new("Two"), 2, NOW).await.unwrap();
    assert!(matches!(s.create_character(a, &new("Three"), 2, NOW).await, Err(Error::TooMany)));
}

#[tokio::test]
async fn concurrent_creates_with_one_name_store_one() {
    let (_d, s) = store().await;
    let a = account(&s, 1, "bob").await;
    let b = account(&s, 2, "eve").await;
    let (x, y) = tokio::join!(
        s.create_character(a, &new("Same"), 4, NOW),
        s.create_character(b, &new("same"), 4, NOW)
    );
    assert_eq!(x.is_ok() as u8 + y.is_ok() as u8, 1);
    assert!(matches!(x.err().or(y.err()), Some(Error::NameTaken)));
}

// accounts.rs tests
#[tokio::test]
async fn deleting_an_account_cascades_but_keeps_its_invite_spent() {
    let (_d, s) = store().await;
    let a = account(&s, 1, "bob").await;
    s.create_character(a, &new("Lin"), 4, NOW).await.unwrap();
    s.create_session(&h(9), a, NOW, NOW + 10).await.unwrap();
    s.create_game_token(&h(10), a, NOW + 10).await.unwrap();
    assert!(s.delete_account("BOB").await.unwrap());
    assert!(s.account_by_username("bob").await.unwrap().is_none());
    assert!(s.list_characters(a).await.unwrap().is_empty());
    assert_eq!(s.session_account(&h(9), NOW).await.unwrap(), None);
    assert_eq!(s.consume_game_token(&h(10), NOW).await.unwrap(), None);
    assert!(matches!(s.redeem_invite(&h(1), "bob", "$p", NOW).await, Err(Error::InviteInvalid)));
    let inv = &s.list_invites().await.unwrap()[0];
    assert!(inv.redeemed_at.is_some() && inv.redeemed_by.is_none());
}

#[tokio::test]
async fn sign_ins_are_recorded_and_accounts_listed() {
    let (_d, s) = store().await;
    let a = account(&s, 1, "bob").await;
    s.record_sign_in(a, NOW + 5).await.unwrap();
    let list = s.list_accounts().await.unwrap();
    assert_eq!((list[0].username.as_str(), list[0].last_sign_in_at, list[0].characters), ("bob", Some(NOW + 5), 0));
    assert_eq!(s.account_by_username("BOB").await.unwrap().unwrap().id, a);
}
```

- [ ] **Step 3: Run `cargo test -p fenji-store`; expect FAIL (missing functions).**

- [ ] **Step 4: Implement.** Each function is one query, or one `self.begin()` transaction where the spec asks for atomicity. Static SQL strings only (sqlx 0.9 takes `SqlSafeStr`).

`src/invites.rs`:
```rust
use sqlx::FromRow;

use crate::error::is_unique;
use crate::{Error, Result, Store};

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct InviteRow {
    pub id: i64,
    pub label: Option<String>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub redeemed_by: Option<String>,
    pub redeemed_at: Option<i64>,
}

impl Store {
    pub async fn create_invite(&self, code_hash: &[u8; 32], label: Option<&str>, now: i64, expires_at: Option<i64>) -> Result<i64> {
        let r = sqlx::query("INSERT INTO invites (code_hash, label, created_at, expires_at) VALUES (?, ?, ?, ?)")
            .bind(&code_hash[..])
            .bind(label)
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await?;
        Ok(r.last_insert_rowid())
    }

    pub async fn list_invites(&self) -> Result<Vec<InviteRow>> {
        Ok(sqlx::query_as(
            "SELECT i.id, i.label, i.created_at, i.expires_at, i.revoked_at, a.username AS redeemed_by, i.redeemed_at \
             FROM invites i LEFT JOIN accounts a ON a.id = i.redeemed_by ORDER BY i.id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn revoke_invite(&self, id: i64, now: i64) -> Result<bool> {
        let r = sqlx::query("UPDATE invites SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL AND redeemed_at IS NULL")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Spends the invite and creates the account in one transaction. The invite is checked first,
    /// so only someone holding a valid invite learns whether a username is taken (M3 spec D12).
    pub async fn redeem_invite(&self, code_hash: &[u8; 32], username: &str, password_phc: &str, now: i64) -> Result<i64> {
        let mut tx = self.begin().await?;
        let invite: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM invites WHERE code_hash = ? AND redeemed_at IS NULL AND revoked_at IS NULL \
             AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(&code_hash[..])
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((invite_id,)) = invite else {
            return Err(Error::InviteInvalid);
        };
        let account = sqlx::query("INSERT INTO accounts (username, password_phc, created_at) VALUES (?, ?, ?)")
            .bind(username)
            .bind(password_phc)
            .bind(now)
            .execute(&mut *tx)
            .await;
        let account_id = match account {
            Ok(r) => r.last_insert_rowid(),
            Err(e) if is_unique(&e) => return Err(Error::UsernameTaken),
            Err(e) => return Err(e.into()),
        };
        sqlx::query("UPDATE invites SET redeemed_by = ?, redeemed_at = ? WHERE id = ?")
            .bind(account_id)
            .bind(now)
            .bind(invite_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(account_id)
    }
}
```

`src/accounts.rs`:
```rust
use sqlx::FromRow;

use crate::{Result, Store};

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Account {
    pub id: i64,
    pub username: String,
    pub password_phc: String,
    pub created_at: i64,
    pub last_sign_in_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct AccountSummary {
    pub id: i64,
    pub username: String,
    pub created_at: i64,
    pub last_sign_in_at: Option<i64>,
    pub characters: i64,
}

impl Store {
    pub async fn account_by_username(&self, username: &str) -> Result<Option<Account>> {
        Ok(sqlx::query_as("SELECT id, username, password_phc, created_at, last_sign_in_at FROM accounts WHERE username = ?")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn account_by_id(&self, id: i64) -> Result<Option<Account>> {
        Ok(sqlx::query_as("SELECT id, username, password_phc, created_at, last_sign_in_at FROM accounts WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn record_sign_in(&self, id: i64, now: i64) -> Result<()> {
        sqlx::query("UPDATE accounts SET last_sign_in_at = ? WHERE id = ?").bind(now).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn list_accounts(&self) -> Result<Vec<AccountSummary>> {
        Ok(sqlx::query_as(
            "SELECT a.id, a.username, a.created_at, a.last_sign_in_at, \
             (SELECT COUNT(*) FROM characters c WHERE c.account_id = a.id) AS characters \
             FROM accounts a ORDER BY a.id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Deletes the account; its characters, sessions and tokens go with it (foreign keys), and
    /// its invite stays spent (PRIVACY.md, "Deleting your data").
    pub async fn delete_account(&self, username: &str) -> Result<bool> {
        let r = sqlx::query("DELETE FROM accounts WHERE username = ?").bind(username).execute(&self.pool).await?;
        Ok(r.rows_affected() == 1)
    }
}
```

`src/tokens.rs`:
```rust
use crate::{Result, Store};

impl Store {
    pub async fn create_session(&self, token_hash: &[u8; 32], account_id: i64, now: i64, expires_at: i64) -> Result<()> {
        sqlx::query("INSERT INTO sessions (token_hash, account_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
            .bind(&token_hash[..]).bind(account_id).bind(now).bind(expires_at)
            .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn session_account(&self, token_hash: &[u8; 32], now: i64) -> Result<Option<i64>> {
        let r: Option<(i64,)> = sqlx::query_as("SELECT account_id FROM sessions WHERE token_hash = ? AND expires_at > ?")
            .bind(&token_hash[..]).bind(now)
            .fetch_optional(&self.pool).await?;
        Ok(r.map(|(a,)| a))
    }

    pub async fn delete_session(&self, token_hash: &[u8; 32]) -> Result<bool> {
        let r = sqlx::query("DELETE FROM sessions WHERE token_hash = ?").bind(&token_hash[..]).execute(&self.pool).await?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn create_game_token(&self, token_hash: &[u8; 32], account_id: i64, expires_at: i64) -> Result<()> {
        sqlx::query("INSERT INTO game_tokens (token_hash, account_id, expires_at) VALUES (?, ?, ?)")
            .bind(&token_hash[..]).bind(account_id).bind(expires_at)
            .execute(&self.pool).await?;
        Ok(())
    }

    /// Spends a game token: one statement, so two connections presenting the same token can't
    /// both get the account (M3 spec D3).
    pub async fn consume_game_token(&self, token_hash: &[u8; 32], now: i64) -> Result<Option<i64>> {
        let r: Option<(i64,)> = sqlx::query_as(
            "UPDATE game_tokens SET used_at = ? WHERE token_hash = ? AND used_at IS NULL AND expires_at > ? RETURNING account_id",
        )
        .bind(now).bind(&token_hash[..]).bind(now)
        .fetch_optional(&self.pool).await?;
        Ok(r.map(|(a,)| a))
    }

    /// Drops expired sessions and every game token that is spent or expired.
    pub async fn purge_expired(&self, now: i64) -> Result<u64> {
        let mut tx = self.begin().await?;
        let s = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?").bind(now).execute(&mut *tx).await?;
        let t = sqlx::query("DELETE FROM game_tokens WHERE used_at IS NOT NULL OR expires_at <= ?").bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(s.rows_affected() + t.rows_affected())
    }
}
```

`src/characters.rs`:
```rust
use sqlx::FromRow;

use crate::error::is_unique;
use crate::{Error, Result, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCharacter {
    pub name: String,
    pub vocation: i64,
    pub skeleton: i64,
    /// The validated appearance tree, as its own pickle (M3 spec D8).
    pub appearance: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Character {
    pub pid: i64,
    pub account_id: i64,
    pub name: String,
    pub vocation: i64,
    pub skeleton: i64,
    pub level: i64,
    pub appearance: Vec<u8>,
    pub created_at: i64,
}

impl Store {
    pub async fn list_characters(&self, account_id: i64) -> Result<Vec<Character>> {
        Ok(sqlx::query_as(
            "SELECT pid, account_id, name, vocation, skeleton, level, appearance, created_at FROM characters \
             WHERE account_id = ? ORDER BY pid",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// The limit and the name check run in the same transaction as the insert.
    pub async fn create_character(&self, account_id: i64, new: &NewCharacter, max_per_account: u32, now: i64) -> Result<Character> {
        let mut tx = self.begin().await?;
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM characters WHERE account_id = ?")
            .bind(account_id)
            .fetch_one(&mut *tx)
            .await?;
        if count >= i64::from(max_per_account) {
            return Err(Error::TooMany);
        }
        let inserted = sqlx::query_as(
            "INSERT INTO characters (account_id, name, vocation, skeleton, appearance, created_at) VALUES (?, ?, ?, ?, ?, ?) \
             RETURNING pid, account_id, name, vocation, skeleton, level, appearance, created_at",
        )
        .bind(account_id)
        .bind(&new.name)
        .bind(new.vocation)
        .bind(new.skeleton)
        .bind(&new.appearance)
        .bind(now)
        .fetch_one(&mut *tx)
        .await;
        let c: Character = match inserted {
            Ok(c) => c,
            Err(e) if is_unique(&e) => return Err(Error::NameTaken),
            Err(e) => return Err(e.into()),
        };
        tx.commit().await?;
        Ok(c)
    }

    /// Deletes `pid` only if it belongs to `account_id`.
    pub async fn delete_character(&self, account_id: i64, pid: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM characters WHERE pid = ? AND account_id = ?").bind(pid).bind(account_id).execute(&self.pool).await?;
        Ok(r.rows_affected() == 1)
    }
}
```

- [ ] **Step 5: Run `cargo test -p fenji-store`; expect PASS.** If `concurrent_creates_with_one_name_store_one` deadlocks or reports `SQLITE_BUSY`, the `BEGIN IMMEDIATE` and the busy timeout are not in effect: fix that, don't loosen the test.

- [ ] **Step 6: Full gate (fmt, clippy, test, `cargo deny check`), commit, push, PR, review, merge.**

```bash
git add Cargo.toml Cargo.lock crates/fenji-store
git commit -m "feat(store): add fenji-store: accounts, invites, tokens and characters in SQLite" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `fenji-proto` lobby messages (W2)

**Files:**
- Create: `crates/fenji-proto/src/lobby.rs`, `crates/fenji-proto/tests/lobby.rs`
- Modify: `crates/fenji-proto/src/lib.rs` (`pub mod lobby;`), `src/pickle.rs` (`encode_compressed`), `src/handshake.rs` (`drain_messages` takes its limit), `src/client.rs` (the simulator's own inbound limit; `time_sync` after close), `crates/fenji-data/src/cache.rs:5-6` (the stale comment, D19)

**Interfaces:**
- Consumes: `docs/protocol/packets/lobby.md` (Task 1), whose findings these items cite:
  - the reading in `re/m3/lobby-reading.md`;
  - `fenji_proto::packet::script_body(i64, &[Value]) -> Result<Vec<u8>>`;
  - `pickle::{encode, decode, Table, Value}`.
- Produces:
  - `fenji_proto::pickle::encode_compressed(&[Value]) -> Result<Vec<u8>>`.
  - `fenji_proto::lobby::{QUERY_ACTORS, SELECT_ACTOR, CREATE_ACTOR, DELETE_ACTOR, CHANGE_SLOT, HOTPATCH_INIT, GM_SCRIPTS, SERVER_LIST, CLIENT_EXIT, ROLE_LIST, ROLE_CREATE, ROLE_DELETE, ERROR_CODE, NEVER_SEND, ROLE_SLOTS, code::*}`.
  - `lobby::{CreateRequest, Appearance, Refusal, Actor, AccountExtras}`, with `CreateRequest::parse(&[Value]) -> Result<CreateRequest, Refusal>`, `Refusal::code(self) -> i64`, `Appearance::to_table(&self) -> Table`, `Appearance::from_table(&Table) -> Appearance` and `Actor::to_table(&self, slot: i64) -> Table`.
  - `lobby::{role_list(i64, &[Actor], &AccountExtras) -> Result<Vec<u8>>, create_result(&Actor, slot: i64) -> Result<Vec<u8>>, delete_result(i64) -> Vec<u8>, error_code(i64) -> Vec<u8>}`.

What the reading established, which this task encodes:
- **Script ids.** The client's own protocol table `data/caches/protocol_ver.new_cache` names every id the scripts use (`lobby-reading.md` Q1):
  - client to server: 292 `PROTOC2S_ROLE_LIST`, 10 `ROLE_SELECT`, 304 `ROLE_CREATE`, 166 `ROLE_DELETE`, 321 `PROTOC2G_ROLE_SLOT`, 370 `PROTOC2G_HOTPATCH_INIT`, 496 `PROTOC2G_GM_SCRIPTS`, 402 `PROTOC2G_SERVER_LIST`, 392 `PROTOC2S_CLIENT_EXIT`;
  - server to client: 228, 628, 84 and 270 `PROTOS2C_ERROR_CODE`.
- **228** is `(last_pid, list, extra)`:
  - `last_pid` is an integer, and 0 selects slot 1.
  - `list` is a sequence, `{}` when empty (never nil).
  - `extra` is a table, `{rename_num, reborn_num, change_gender_num, role_num}`; it can't be nil, and `billing_gap` is optional.
- **An actor table** (Q3):
  - Required: `pid` (a non-zero integer) and `prop_base_vocation` (0–9). Send `prop_base_skeleton` and `slot` (1..n).
  - Never send `enable`, or a `block_cd` above 0.
  - Ten `prop_sys_show_equip_*` flags and `prop_sys_show_exterior` default to false and hide the outfit, so send them as `true`.
- **The create request (304)** is one table of scalars (Q4):
  - `prop_base_name` (a string);
  - `prop_base_vocation`, which the Western screen offers as 0–7 and 9;
  - `prop_base_skeleton`, which vocations 0–5 take as 1 or 2, vocation 6 as 1, and vocations 7 and 9 as 3;
  - `prop_sys_player_voice`, which may be absent;
  - `prop_sys_avatar_feature` (1–2, which may be absent);
  - `prop_sys_avatar_feature_face` (a string `v:5;…`, which may be absent);
  - `prop_sys_player_tattoo_pos` and `prop_sys_player_birthday` (both 0);
  - `prop_base_equips_id` (1 or 2);
  - `prop_sys_portrait` (`portrait_id << 16`).

  It has no gender: the skeleton implies it (1 → 0, otherwise 1).
- **Names** (Q5): the client refuses empty names and the ASCII space, and its box holds 24 bytes. The Western server's rule, in the English text of `ERR_ROLE_NAME_ALLCHINESE`, allows letters a–z, umlauts, grave and acute accents, cedillas, full stops (not first or last), hyphens and underscores.
- **Replies the client needs:**
  - every 304 gets a 628 or a 270, since the create button waits with no timeout (Q6);
  - every 166 gets an 84 or a 270, and 84 `(pid, 0, 2147483647)` shows no pending dialog (Q7);
  - 10 gets a 270, and `ERR_SERVER_NOT_RUNNING` fits best (Q10).
- **Never sent** (§5): 556, 243, 216, 627, 729, 720 and 598; nor 228 or 628 unsolicited.

- [ ] **Step 1: Write the failing tests for `encode_compressed`** (`pickle.rs` tests):

```rust
#[test]
fn encode_compressed_matches_the_clients_rule() {
    let small = vec![Value::Str(b"short".to_vec())];
    assert_eq!(encode_compressed(&small).unwrap(), encode(&small).unwrap(), "under 0x80: plain");
    let big = vec![Value::Str(vec![b'a'; 300])];
    let z = encode_compressed(&big).unwrap();
    assert_eq!(z[0], 0xFF);
    let raw = encode(&big).unwrap();
    assert_eq!(u32::from_be_bytes([0, z[1], z[2], z[3]]) as usize, raw.len());
    assert!(z.len() < raw.len());
    assert_eq!(decode(&z).unwrap(), big);
}

#[test]
fn encode_compressed_keeps_the_plain_form_when_lz4_doesnt_help() {
    let noise: Vec<u8> = (0..200u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
    let v = vec![Value::Str(noise)];
    assert_eq!(encode_compressed(&v).unwrap(), encode(&v).unwrap());
}
```

- [ ] **Step 2: Run (`cargo test -p fenji-proto pickle`); expect FAIL. Implement in `pickle.rs`:**

```rust
/// What the client's packers do with a finished packet (FUN_1400D53D0, payloads.md P.5): from
/// 0x80 bytes up, LZ4-compress it and keep the result when it and its 4-byte header are shorter:
/// `FF`, the raw size as a 24-bit big-endian number, then the block (P.1). Fenji's server writes
/// plain packets; this is for the client simulator, so tests send what the real client sends.
pub fn encode_compressed(values: &[Value]) -> Result<Vec<u8>> {
    let plain = encode(values)?;
    if plain.len() < 0x80 || plain.len() > MAX_UNZIPPED {
        return Ok(plain);
    }
    let block = lz4_flex::block::compress(&plain);
    if block.len() + 4 >= plain.len() {
        return Ok(plain);
    }
    let mut out = Vec::with_capacity(block.len() + 4);
    out.push(0xFF);
    out.extend_from_slice(&(plain.len() as u32).to_be_bytes()[1..]);
    out.extend_from_slice(&block);
    Ok(out)
}
```

- [ ] **Step 3: Run; expect PASS.**

- [ ] **Step 4: Write the failing tests for `lobby`** (`tests/lobby.rs`):

```rust
use fenji_proto::lobby::{self, AccountExtras, Actor, Appearance, CreateRequest, Refusal, code};
use fenji_proto::pickle::{self, Table, Value};

fn s(x: &str) -> Value {
    Value::Str(x.as_bytes().to_vec())
}

fn get<'a>(t: &'a Table, k: &str) -> Option<&'a Value> {
    t.hash.iter().find(|(kk, _)| *kk == s(k)).map(|(_, v)| v)
}

/// The create request as `CreatePanel.luac` builds it (lobby.md §5).
pub fn request(name: &str, vocation: i64, skeleton: i64) -> Vec<Value> {
    vec![Value::Table(Table {
        array: Vec::new(),
        hash: vec![
            (s("prop_base_name"), s(name)),
            (s("prop_base_vocation"), Value::Int(vocation)),
            (s("prop_base_skeleton"), Value::Int(skeleton)),
            (s("prop_sys_player_voice"), Value::Int(4)),
            (s("prop_sys_avatar_feature"), Value::Int(2)),
            (s("prop_sys_avatar_feature_face"), s("v:5;2:1,2,3;3:1,2,3,4,5;4:1,2,3,4")),
            (s("prop_sys_player_tattoo_pos"), Value::Int(0)),
            (s("prop_sys_player_birthday"), Value::Int(0)),
            (s("prop_base_equips_id"), Value::Int(2)),
            (s("prop_sys_portrait"), Value::Int(24 << 16)),
            (s("something_else"), Value::Int(9)),
        ],
    })]
}

#[test]
fn a_create_request_parses_into_typed_fields() {
    let r = CreateRequest::parse(&request("Lin", 0, 2)).unwrap();
    assert_eq!((r.name.as_str(), r.vocation, r.skeleton), ("Lin", 0, 2));
    assert_eq!(r.appearance.face.as_deref(), Some("v:5;2:1,2,3;3:1,2,3,4,5;4:1,2,3,4"));
    assert_eq!((r.appearance.base_face, r.appearance.equips_id, r.appearance.portrait, r.appearance.voice), (2, 2, 24 << 16, Some(4)));
}

#[test]
fn names_follow_the_western_rule() {
    for ok in ["Lin", "Zoë", "Ça_va-bien", "a.b", &"x".repeat(24)] {
        assert!(CreateRequest::parse(&request(ok, 0, 1)).is_ok(), "{ok}");
    }
    assert_eq!(CreateRequest::parse(&request("", 0, 1)).unwrap_err(), Refusal::NameLength);
    assert_eq!(CreateRequest::parse(&request("L", 0, 1)).unwrap_err(), Refusal::NameLength);
    assert_eq!(CreateRequest::parse(&request(&"x".repeat(25), 0, 1)).unwrap_err(), Refusal::NameLength);
    assert_eq!(CreateRequest::parse(&request(&"ü".repeat(13), 0, 1)).unwrap_err(), Refusal::NameLength, "26 bytes");
    for bad in ["a b", "Lin2", ".Lin", "Lin.", "林", "Li\u{0}n"] {
        assert_eq!(CreateRequest::parse(&request(bad, 0, 1)).unwrap_err(), Refusal::NameChars, "{bad:?}");
    }
    assert_eq!(Refusal::NameChars.code(), code::ROLE_NAME_ALLCHINESE);
}

#[test]
fn vocation_and_skeleton_must_match_the_western_screen() {
    for (v, sk) in [(0, 1), (5, 2), (6, 1), (7, 3), (9, 3)] {
        assert!(CreateRequest::parse(&request("Lin", v, sk)).is_ok(), "{v} {sk}");
    }
    for (v, sk) in [(8, 1), (6, 2), (7, 1), (9, 2), (10, 1), (-1, 1), (0, 3)] {
        assert_eq!(CreateRequest::parse(&request("Lin", v, sk)).unwrap_err(), Refusal::VocationSkeleton, "{v} {sk}");
    }
}

#[test]
fn shapes_no_client_sends_are_refused() {
    assert_eq!(CreateRequest::parse(&[]).unwrap_err(), Refusal::Malformed);
    assert_eq!(CreateRequest::parse(&[Value::Int(1)]).unwrap_err(), Refusal::Malformed);
    let mut r = request("Lin", 0, 1);
    if let Value::Table(t) = &mut r[0] {
        t.hash[5].1 = Value::Table(Table::default()); // a table where the face string goes
    }
    assert_eq!(CreateRequest::parse(&r).unwrap_err(), Refusal::Malformed);
    let mut r = request("Lin", 0, 1);
    if let Value::Table(t) = &mut r[0] {
        t.hash[5].1 = s(&"9".repeat(5000)); // a face string over 4 KiB
    }
    assert_eq!(CreateRequest::parse(&r).unwrap_err(), Refusal::Malformed);
}

#[test]
fn out_of_range_appearance_values_are_clamped_as_the_client_does() {
    let mut r = request("Lin", 0, 1);
    if let Value::Table(t) = &mut r[0] {
        t.hash[4].1 = Value::Int(7); // base face
        t.hash[8].1 = Value::Int(9); // outfit
    }
    let a = CreateRequest::parse(&r).unwrap().appearance;
    assert_eq!((a.base_face, a.equips_id), (1, 1));
}

#[test]
fn appearance_round_trips_through_its_table() {
    let a = CreateRequest::parse(&request("Lin", 0, 2)).unwrap().appearance;
    assert_eq!(Appearance::from_table(&a.to_table()), a);
}

fn lin() -> Actor {
    Actor { pid: 7, name: "Lin".into(), level: 1, vocation: 0, skeleton: 2, appearance: CreateRequest::parse(&request("Lin", 0, 2)).unwrap().appearance }
}

#[test]
fn an_actor_table_has_what_the_select_screen_reads() {
    let t = lin().to_table(3);
    assert_eq!(get(&t, "pid"), Some(&Value::Int(7)));
    assert_eq!(get(&t, "slot"), Some(&Value::Int(3)));
    assert_eq!(get(&t, "prop_base_name"), Some(&s("Lin")));
    assert_eq!(get(&t, "prop_base_lv"), Some(&Value::Int(1)));
    assert_eq!(get(&t, "prop_base_vocation"), Some(&Value::Int(0)));
    assert_eq!(get(&t, "prop_base_skeleton"), Some(&Value::Int(2)));
    assert_eq!(get(&t, "prop_base_gender"), Some(&Value::Int(1)));
    assert_eq!(get(&t, "prop_sys_portrait"), Some(&Value::Int(24 << 16)));
    assert_eq!(get(&t, "prop_sys_avatar_feature"), Some(&Value::Int(2)));
    assert!(matches!(get(&t, "prop_sys_avatar_feature_face"), Some(Value::Str(_))));
    assert_eq!(get(&t, "prop_sys_equips"), Some(&Value::Table(Table::default())));
    for flag in lobby::SHOW_FLAGS {
        assert_eq!(get(&t, flag), Some(&Value::Bool(true)), "{flag}");
    }
    for never in ["enable", "block_cd", "delete_cd"] {
        assert!(get(&t, never).is_none(), "{never}");
    }
}

#[test]
fn role_list_is_last_pid_list_and_extras() {
    let extras = AccountExtras { role_num: 0 };
    let v = pickle::decode(&lobby::role_list(0, &[], &extras).unwrap()).unwrap();
    assert_eq!(v[0], Value::Int(228));
    assert_eq!(v[1], Value::Int(0));
    assert_eq!(v[2], Value::Table(Table::default()), "an empty list is a table");
    let Value::Table(extra) = &v[3] else { panic!("extra must be a table") };
    for k in ["rename_num", "reborn_num", "change_gender_num", "role_num"] {
        assert_eq!(get(extra, k), Some(&Value::Int(0)), "{k}");
    }
    let v = pickle::decode(&lobby::role_list(7, &[lin(), lin()], &extras).unwrap()).unwrap();
    let Value::Table(list) = &v[2] else { panic!() };
    let Value::Table(second) = &list.array[1] else { panic!() };
    assert_eq!(get(second, "slot"), Some(&Value::Int(2)), "slots follow list order");
}

#[test]
fn create_delete_and_error_messages() {
    let v = pickle::decode(&lobby::create_result(&lin(), 1).unwrap()).unwrap();
    assert_eq!(v[0], Value::Int(628));
    assert!(matches!(&v[1], Value::Table(t) if get(t, "pid") == Some(&Value::Int(7))));
    assert_eq!(pickle::decode(&lobby::delete_result(7)).unwrap(), vec![Value::Int(84), Value::Int(7), Value::Int(0), Value::Int(2_147_483_647)]);
    assert_eq!(pickle::decode(&lobby::error_code(code::SERVER_NOT_RUNNING)).unwrap(), vec![Value::Int(270), Value::Int(7_248_304_305_738_093_764)]);
}

#[test]
fn nothing_the_lobby_builds_is_on_the_never_send_list() {
    for id in [lobby::ROLE_LIST, lobby::ROLE_CREATE, lobby::ROLE_DELETE, lobby::ERROR_CODE] {
        assert!(!lobby::NEVER_SEND.contains(&id));
    }
}
```

- [ ] **Step 5: Run; expect FAIL. Implement `src/lobby.rs`** (doc comments cite `lobby.md` by finding number, written in Task 1; the numbers below are the reading's question numbers until then):

```rust
//! The lobby's script messages (docs/protocol/packets/lobby.md): the character list, creation and
//! deletion and the error message, as the client's own scripts send and handle them. Ids are the
//! literals in `data/core/modules/net/net.luac`, which the client's protocol table
//! (`data/caches/protocol_ver.new_cache`) names; refusals carry `ID`s from the client's own `errors`
//! sheet. Building and parsing only: the server decides.
use crate::Result;
use crate::packet::script_body;
use crate::pickle::{Table, Value};

/// `PROTOC2S_ROLE_LIST`: `QueryActors()`, after 370 and 496 (net.luac `0.48`).
pub const QUERY_ACTORS: i64 = 292;
/// `PROTOC2S_ROLE_SELECT (pid)`: entering the world, M4 (`0.49`).
pub const SELECT_ACTOR: i64 = 10;
/// `PROTOC2S_ROLE_CREATE (table)` (`0.50`).
pub const CREATE_ACTOR: i64 = 304;
/// `PROTOC2S_ROLE_DELETE (pid)` (`0.51`).
pub const DELETE_ACTOR: i64 = 166;
/// `PROTOC2G_ROLE_SLOT (pid, slot[, other pid])`: the select screen renumbers or swaps (`0.54`).
pub const CHANGE_SLOT: i64 = 321;
/// `PROTOC2G_HOTPATCH_INIT (revision)` (hotpatch_system.luac `0.4`); never answered.
pub const HOTPATCH_INIT: i64 = 370;
/// `PROTOC2G_GM_SCRIPTS ()`; never answered.
pub const GM_SCRIPTS: i64 = 496;
/// `PROTOC2G_SERVER_LIST ()`, sent by the 414 handler (net.luac `0.29`); its reply 495 is cosmetic.
pub const SERVER_LIST: i64 = 402;
/// `PROTOC2S_CLIENT_EXIT`: the client is quitting.
pub const CLIENT_EXIT: i64 = 392;

/// `PROTOG2C_ROLE_LIST (last_pid, list, extra)` (net.luac `0.32`).
pub const ROLE_LIST: i64 = 228;
/// `PROTOG2C_ROLE_CREATE (actor)` (`0.33`).
pub const ROLE_CREATE: i64 = 628;
/// `PROTOG2C_ROLE_DELETE (pid, ms, level)` (`0.34`).
pub const ROLE_DELETE: i64 = 84;
/// `PROTOS2C_ERROR_CODE (code)` (err.luac `0.8`).
pub const ERROR_CODE: i64 = 270;
/// Never sent (ground rule 6; lobby.md §2): 556 runs Lua bytecode, 243 and 216 replace scripts and
/// layouts, 627 patches tables, 729 writes a file to the player's disk, 720 and 598 open URLs.
pub const NEVER_SEND: [i64; 7] = [556, 243, 216, 627, 729, 720, 598];
/// The select screen shows `9 + role_num` places (AS/ActorSelect.luac `0.3`).
pub const ROLE_SLOTS: u32 = 9;

/// Rows of the client's `errors` sheet, by `literal` (lobby.md §8).
pub mod code {
    pub const ROLE_NAME_EXIST: i64 = 5_391_046_069_930_669_593;
    pub const ROLE_NAME_LENERR: i64 = -2_076_552_961_395_212_067;
    pub const ROLE_NAME_ILLEGAL: i64 = -2_195_789_287_712_819_046;
    /// Its English text lists the Western server's permitted name characters.
    pub const ROLE_NAME_ALLCHINESE: i64 = -2_789_480_703_151_742_396;
    pub const ROLE_MAX_NUMBER: i64 = 8_663_116_806_817_837_223;
    pub const ROLE_NOT_FOUND: i64 = -2_776_944_664_988_636_544;
    pub const ROLE_VOC_SKELETON_NOT_MATCH: i64 = -7_591_047_980_372_978_664;
    /// "The server isn't open yet": M3's answer to entering the world.
    pub const SERVER_NOT_RUNNING: i64 = 7_248_304_305_738_093_764;
    /// "Server not responding": for the server's own failures.
    pub const SERVER_NOT_RESP: i64 = -5_759_203_401_640_685_215;
}

/// The model's show flags that hide their piece when missing (`x or false`, pregame_actor.luac
/// `0.0`): sent as `true`.
pub const SHOW_FLAGS: [&str; 11] = [
    "prop_sys_show_equip_head",
    "prop_sys_show_equip_artifact1",
    "prop_sys_show_equip_artifact2",
    "prop_sys_show_equip_chest",
    "prop_sys_show_equip_wrist",
    "prop_sys_show_equip_boot",
    "prop_sys_show_equip_weapon",
    "prop_sys_show_equip_pendantmoon",
    "prop_sys_show_equip_pendantstar1",
    "prop_sys_show_equip_pendantstar2",
    "prop_sys_show_exterior",
];

/// A name's length in bytes of UTF-8: the client's box holds 24 (FeatureSetting.xml `maxBytes`).
pub const NAME_BYTES: std::ops::RangeInclusive<usize> = 2..=24;
/// The face string's limit: the client's own runs to about 1–2 KiB (lobby.md §5).
pub const FACE_MAX: usize = 4096;

/// Why a create request is refused, and the client's code for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NameLength,
    NameChars,
    VocationSkeleton,
    Malformed,
}

impl Refusal {
    pub fn code(self) -> i64 {
        match self {
            Self::NameLength => code::ROLE_NAME_LENERR,
            Self::NameChars => code::ROLE_NAME_ALLCHINESE,
            Self::VocationSkeleton => code::ROLE_VOC_SKELETON_NOT_MATCH,
            Self::Malformed => code::ROLE_NAME_ILLEGAL,
        }
    }
}

/// The appearance fields the server keeps, typed and bounded (M3 spec D8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appearance {
    pub voice: Option<i64>,
    /// The base face, 1 or 2 (`check_base_face` clamps to that, default 1).
    pub base_face: i64,
    pub face: Option<String>,
    /// The starting outfit, 1 or 2.
    pub equips_id: i64,
    /// `portrait_id << 16 | frame`.
    pub portrait: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRequest {
    pub name: String,
    pub vocation: i64,
    pub skeleton: i64,
    pub appearance: Appearance,
}

fn key(k: &Value) -> Option<&str> {
    match k {
        Value::Str(b) => std::str::from_utf8(b).ok(),
        _ => None,
    }
}

fn int(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(*f as i64),
        _ => None,
    }
}

/// The skeletons the Western create screen offers per vocation (`vocation_skel_templ`; lobby.md §5).
pub fn skeleton_allowed(vocation: i64, skeleton: i64) -> bool {
    matches!((vocation, skeleton), (0..=5, 1 | 2) | (6, 1) | (7 | 9, 3))
}

/// The Western server's characters (ERR_ROLE_NAME_ALLCHINESE's English text): letters a–z, the
/// Latin-1 letters with umlauts, accents and cedillas, full stops (not first or last), hyphens and
/// underscores.
fn name_chars_ok(name: &str) -> bool {
    let ok = |c: char| c.is_ascii_alphabetic() || matches!(c, '\u{C0}'..='\u{FF}' if c != '\u{D7}' && c != '\u{F7}') || matches!(c, '.' | '-' | '_');
    name.chars().all(ok) && !name.starts_with('.') && !name.ends_with('.')
}

impl CreateRequest {
    /// `args` are message 304's arguments: one table of scalars (lobby.md §5).
    pub fn parse(args: &[Value]) -> std::result::Result<Self, Refusal> {
        let Some(Value::Table(t)) = args.first() else {
            return Err(Refusal::Malformed);
        };
        let (mut name, mut vocation, mut skeleton) = (None, None, None);
        let mut a = Appearance { voice: None, base_face: 1, face: None, equips_id: 1, portrait: 25 << 16 };
        for (k, v) in &t.hash {
            match key(k) {
                Some("prop_base_name") => match v {
                    Value::Str(b) => name = Some(String::from_utf8(b.clone()).map_err(|_| Refusal::NameChars)?),
                    _ => return Err(Refusal::Malformed),
                },
                Some("prop_base_vocation") => vocation = Some(int(v).ok_or(Refusal::Malformed)?),
                Some("prop_base_skeleton") => skeleton = Some(int(v).ok_or(Refusal::Malformed)?),
                Some("prop_sys_player_voice") => a.voice = Some(int(v).filter(|x| (0..=64).contains(x)).ok_or(Refusal::Malformed)?),
                Some("prop_sys_avatar_feature") => a.base_face = int(v).filter(|x| (1..=2).contains(x)).unwrap_or(1),
                Some("prop_sys_avatar_feature_face") => match v {
                    Value::Str(b) if b.len() <= FACE_MAX && b.iter().all(|c| c.is_ascii_graphic()) => {
                        a.face = Some(String::from_utf8(b.clone()).map_err(|_| Refusal::Malformed)?)
                    }
                    _ => return Err(Refusal::Malformed),
                },
                Some("prop_base_equips_id") => a.equips_id = int(v).filter(|x| (1..=2).contains(x)).unwrap_or(1),
                Some("prop_sys_portrait") => a.portrait = int(v).filter(|x| (0..=i64::from(i32::MAX)).contains(x)).ok_or(Refusal::Malformed)?,
                Some("prop_sys_player_tattoo_pos" | "prop_sys_player_birthday") => {
                    int(v).ok_or(Refusal::Malformed)?;
                }
                _ => {}
            }
        }
        let name = name.ok_or(Refusal::Malformed)?;
        if !NAME_BYTES.contains(&name.len()) {
            return Err(Refusal::NameLength);
        }
        if !name_chars_ok(&name) {
            return Err(Refusal::NameChars);
        }
        let (vocation, skeleton) = (vocation.ok_or(Refusal::Malformed)?, skeleton.ok_or(Refusal::Malformed)?);
        if !skeleton_allowed(vocation, skeleton) {
            return Err(Refusal::VocationSkeleton);
        }
        Ok(Self { name, vocation, skeleton, appearance: a })
    }
}

fn s(x: &str) -> Value {
    Value::Str(x.as_bytes().to_vec())
}

impl Appearance {
    /// As the server stores it: a flat table, encoded with `pickle::encode`.
    pub fn to_table(&self) -> Table {
        let mut hash = vec![
            (s("base_face"), Value::Int(self.base_face)),
            (s("equips_id"), Value::Int(self.equips_id)),
            (s("portrait"), Value::Int(self.portrait)),
        ];
        if let Some(v) = self.voice {
            hash.push((s("voice"), Value::Int(v)));
        }
        if let Some(f) = &self.face {
            hash.push((s("face"), s(f)));
        }
        Table { array: Vec::new(), hash }
    }

    /// The stored table back; anything missing takes the create screen's default.
    pub fn from_table(t: &Table) -> Self {
        let get = |k: &str| t.hash.iter().find(|(kk, _)| key(kk) == Some(k)).map(|(_, v)| v);
        Self {
            voice: get("voice").and_then(int),
            base_face: get("base_face").and_then(int).unwrap_or(1),
            face: get("face").and_then(|v| match v { Value::Str(b) => String::from_utf8(b.clone()).ok(), _ => None }),
            equips_id: get("equips_id").and_then(int).unwrap_or(1),
            portrait: get("portrait").and_then(int).unwrap_or(25 << 16),
        }
    }
}

/// One character as the select screen reads it (lobby.md §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub pid: i64,
    pub name: String,
    pub level: i64,
    pub vocation: i64,
    pub skeleton: i64,
    pub appearance: Appearance,
}

impl Actor {
    pub fn to_table(&self, slot: i64) -> Table {
        let mut hash = vec![
            (s("pid"), Value::Int(self.pid)),
            (s("slot"), Value::Int(slot)),
            (s("prop_base_name"), s(&self.name)),
            (s("prop_base_lv"), Value::Int(self.level)),
            (s("prop_base_vocation"), Value::Int(self.vocation)),
            (s("prop_base_gender"), Value::Int(if self.skeleton == 1 { 0 } else { 1 })),
            (s("prop_base_skeleton"), Value::Int(self.skeleton)),
            (s("prop_sys_portrait"), Value::Int(self.appearance.portrait)),
            (s("prop_sys_avatar_feature"), Value::Int(self.appearance.base_face)),
            // No outfit yet: its items come from the client's tables (D8, M4).
            (s("prop_sys_equips"), Value::Table(Table::default())),
        ];
        if let Some(f) = &self.appearance.face {
            hash.push((s("prop_sys_avatar_feature_face"), s(f)));
        }
        hash.extend(SHOW_FLAGS.iter().map(|f| (s(f), Value::Bool(true))));
        Table { array: Vec::new(), hash }
    }
}

/// 228's third argument (lobby.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountExtras {
    /// Places beyond the screen's nine.
    pub role_num: i64,
}

/// 228: the pid to pre-select (0: slot 1), the actors with slots in list order, the extras.
pub fn role_list(last_pid: i64, actors: &[Actor], extras: &AccountExtras) -> Result<Vec<u8>> {
    let list = Table { array: actors.iter().zip(1..).map(|(a, slot)| Value::Table(a.to_table(slot))).collect(), hash: Vec::new() };
    let extra = Table {
        array: Vec::new(),
        hash: vec![
            (s("rename_num"), Value::Int(0)),
            (s("reborn_num"), Value::Int(0)),
            (s("change_gender_num"), Value::Int(0)),
            (s("role_num"), Value::Int(extras.role_num)),
        ],
    };
    script_body(ROLE_LIST, &[Value::Int(last_pid), Value::Table(list), Value::Table(extra)])
}

/// 628: the new actor, in the slot after the account's others (lobby.md §6).
pub fn create_result(actor: &Actor, slot: i64) -> Result<Vec<u8>> {
    script_body(ROLE_CREATE, &[Value::Table(actor.to_table(slot))])
}

/// 84: the pid, no pending time, and a level threshold no character reaches, so the client shows
/// no "pending deletion" dialog and simply asks for the list again (lobby.md §7; M3 spec D9).
pub fn delete_result(pid: i64) -> Vec<u8> {
    script_body(ROLE_DELETE, &[Value::Int(pid), Value::Int(0), Value::Int(2_147_483_647)]).expect("four integers")
}

/// 270 with one of [`code`]'s values.
pub fn error_code(code: i64) -> Vec<u8> {
    script_body(ERROR_CODE, &[Value::Int(code)]).expect("two integers")
}
```

- [ ] **Step 6: Run; expect PASS.**

- [ ] **Step 7: The simulator and the deferred minors (D19).**
  - `handshake::drain_messages` gets a `limit: usize` parameter. The server passes `MAX_INBOUND`.
  - The client simulator passes `CLIENT_MAX_MESSAGE = 0xFF_FFFF`: the largest length the 24-bit header can hold, since the real client keeps no smaller limit of its own for server messages (transport.md 7.1).
  - `Client::time_sync` returns an empty `Vec` once the client is `Closed`.
  - Fix the stale comment in `crates/fenji-data/src/cache.rs` lines 5–6: `fenji-proto` reads tags 0D–10 since v0.3.1.
  - The ref-cycle minor needs no code: no M3 code follows table references, since the create request holds only scalars (Q4).

```rust
// crates/fenji-proto/tests/handshake.rs, next to `oversized_inbound_message_is_refused`
#[test]
fn the_simulator_takes_server_messages_larger_than_the_servers_own_limit() {
    let (mut server, mut client) = to_work(41);
    let big = vec![7u8; MAX_INBOUND + 1];
    let bytes = server.send(0, &big, &StoredCodec).unwrap();
    let (events, _) = client.receive(&bytes).unwrap();
    assert!(matches!(&events[..], [ClientEvent::Message(m)] if m.body.len() == big.len()));
}

#[test]
fn a_closed_simulator_sends_no_time_syncs() {
    let (mut server, mut client) = to_work(42);
    let close = server.send(1, b"bye", &StoredCodec).unwrap();
    client.receive(&close).unwrap();
    assert!(client.time_sync(5).is_empty());
}
```

- [ ] **Step 8: Full gate; commit; PR; review; merge.**

```bash
git add crates/fenji-proto crates/fenji-data/src/cache.rs
git commit -m "feat(proto): type the lobby's messages and check what clients ask to store" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: `fenji-server`: config, accounts, API and admin commands (W5)

**Files:**
- Create: `crates/fenji-server/Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/config.rs`, `src/clock.rs`, `src/accounts.rs`, `src/api.rs`, `src/cli.rs`, `crates/fenji-server/fenji.example.toml`, `crates/fenji-server/tests/api.rs`, `tests/cli.rs`
- Modify: `Cargo.toml` (member; `axum = { version = "0.8.9", default-features = false, features = ["http1", "json", "tokio", "matched-path"] }`, `tower = { version = "0.5.3", default-features = false, features = ["util"] }`, `tower-http = { version = "0.7.1", features = ["timeout"] }`, `serde = { version = "1.0.229", features = ["derive"] }`, `serde_json = "1.0.154"`, `toml = "1.1.8"`; `tokio` gains `sync`), `Cargo.lock`

**Interfaces:**
- Consumes: Task 2's `fenji_auth::*`; Task 3's `fenji_store::*`.
- Produces:
  - `fenji_server::config::{Config, ServerConfig, StoreConfig, DataConfig, AuthConfig, LobbyConfig, AccessConfig}`, with `Config::load(&Path) -> anyhow::Result<Config>` and `Config::parse(&str, base: &Path) -> anyhow::Result<Config>`.
  - `fenji_server::clock::{Clock, SystemClock, TestClock}`.
  - `fenji_server::accounts::{Accounts, AccountsError, SessionGrant, GameGrant}`.
  - `fenji_server::api::{router(AppState) -> axum::Router, AppState, ServerInfo}`.
  - `fenji_server::cli::{Cli, Command, admin}`.

- [ ] **Step 1: Write the failing config tests** (`config.rs` tests):

```rust
const EXAMPLE: &str = include_str!("../fenji.example.toml");

#[test]
fn the_example_parses_and_resolves_the_store_path() {
    let c = Config::parse(EXAMPLE, Path::new("/etc/fenji")).unwrap();
    assert_eq!(c.server.login_bind.port(), 7800);
    assert_eq!(c.store.path, Path::new("/etc/fenji/fenji.db"));
    assert_eq!(c.auth.game_token_ttl_secs, 60);
}

#[test]
fn unknown_keys_and_bad_values_are_refused_with_their_place() {
    let e = Config::parse("[server]\nname = \"F\"\nbogus = 1\n", Path::new("/")).unwrap_err().to_string();
    assert!(e.contains("bogus"), "{e}");
    let e = Config::parse("[server]\nname = \"Fen:ji\"\n", Path::new("/")).unwrap_err().to_string();
    assert!(e.contains("colon"), "{e}");
    let e = Config::parse("[server]\nname = \"F\"\nmax_players = 0\n", Path::new("/")).unwrap_err().to_string();
    assert!(e.contains("max_players"), "{e}");
    let e = Config::parse("[server]\nname = \"F\"\n[access]\ninvite_only = false\n", Path::new("/")).unwrap_err().to_string();
    assert!(e.contains("invite_only"), "{e}");
}

#[test]
fn defaults_fill_everything_but_the_name() {
    let c = Config::parse("[server]\nname = \"Fenji\"\n", Path::new("/srv")).unwrap();
    assert_eq!((c.server.id, c.server.max_players, c.lobby.max_characters), (1, 20, LobbyConfig::default().max_characters));
    assert!(c.server.trusted_proxies.is_empty());
}
```

- [ ] **Step 2: Implement `config.rs` and `fenji.example.toml`; run the tests until they pass.**

`fenji.example.toml`:
```toml
# Fenji server configuration (docs/superpowers/specs/2026-10-12-m3-login-lobby-design.md, D16).
# Changes take effect on restart.

[server]
id = 1                          # sent to the client as gj_uinfo's first field
name = "Fenji"                  # shown by the client; no colons
public_host = "127.0.0.1"       # what launchers connect to: a name with an IPv4 address, or one
login_bind = "127.0.0.1:7800"   # the gate (game connections)
api_bind = "127.0.0.1:7801"     # the account API; put Caddy (TLS) in front of it
max_players = 20
trusted_proxies = []            # e.g. ["127.0.0.1"] when Caddy runs on this machine

[store]
path = "fenji.db"               # relative to this file

[data]
# client_path = "/srv/fenji/solo-client"   # your own copy of the pinned client (M3 spec D8)

[auth]
game_token_ttl_secs = 60
session_ttl_secs = 86400

[lobby]
max_characters = 9             # the select screen shows 9 places, more scroll

[access]
invite_only = true
admins = []
```
(`max_characters` defaults to 9, the places the select screen shows without extra slots: `lobby::ROLE_SLOTS`. Above 9, the lobby sends `role_num = max_characters - 9` so the screen shows the extra places.)

`config.rs`:
```rust
//! `fenji.toml` (M3 spec D16): parsed whole, checked, then used. Unknown keys and impossible values
//! are refused with the place they were found.
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub store: StoreConfig,
    #[serde(default)]
    pub data: DataConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub lobby: LobbyConfig,
    #[serde(default)]
    pub access: AccessConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "one")]
    pub id: u32,
    pub name: String,
    #[serde(default = "localhost")]
    pub public_host: String,
    #[serde(default = "login_bind")]
    pub login_bind: SocketAddr,
    #[serde(default = "api_bind")]
    pub api_bind: SocketAddr,
    #[serde(default = "twenty")]
    pub max_players: u32,
    #[serde(default)]
    pub trusted_proxies: Vec<IpAddr>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreConfig {
    pub path: PathBuf,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self { path: PathBuf::from("fenji.db") }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataConfig {
    pub client_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    pub game_token_ttl_secs: u64,
    pub session_ttl_secs: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self { game_token_ttl_secs: 60, session_ttl_secs: 86_400 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LobbyConfig {
    pub max_characters: u32,
}

impl Default for LobbyConfig {
    fn default() -> Self {
        Self { max_characters: 9 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessConfig {
    pub invite_only: bool,
    #[serde(default)]
    pub admins: Vec<String>,
}

impl Default for AccessConfig {
    fn default() -> Self {
        Self { invite_only: true, admins: Vec::new() }
    }
}

fn one() -> u32 { 1 }
fn twenty() -> u32 { 20 }
fn localhost() -> String { "127.0.0.1".into() }
fn login_bind() -> SocketAddr { "127.0.0.1:7800".parse().expect("constant") }
fn api_bind() -> SocketAddr { "127.0.0.1:7801".parse().expect("constant") }

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("can't read {}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        Self::parse(&text, base).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str, base: &Path) -> anyhow::Result<Self> {
        let mut c: Config = toml::from_str(text)?;
        if c.store.path.is_relative() {
            c.store.path = base.join(&c.store.path);
        }
        c.check()?;
        Ok(c)
    }

    fn check(&self) -> anyhow::Result<()> {
        let s = &self.server;
        if s.name.trim().is_empty() || s.name.chars().count() > 32 {
            bail!("[server] name: 1 to 32 characters");
        }
        if s.name.contains(':') {
            bail!("[server] name: no colon (the client splits gj_uinfo on colons)");
        }
        if s.id == 0 || s.id > i32::MAX as u32 {
            bail!("[server] id: 1 to {}", i32::MAX);
        }
        if s.max_players == 0 {
            bail!("[server] max_players: at least 1");
        }
        if s.public_host.trim().is_empty() || s.public_host.contains(':') {
            bail!("[server] public_host: a host name or an IPv4 address, without a port");
        }
        if !(10..=3600).contains(&self.auth.game_token_ttl_secs) {
            bail!("[auth] game_token_ttl_secs: 10 to 3600");
        }
        if !(60..=30 * 86_400).contains(&self.auth.session_ttl_secs) {
            bail!("[auth] session_ttl_secs: 60 to 2592000");
        }
        if self.lobby.max_characters == 0 || self.lobby.max_characters > crate::MAX_CHARACTER_SLOTS {
            bail!("[lobby] max_characters: 1 to {}", crate::MAX_CHARACTER_SLOTS);
        }
        if !self.access.invite_only {
            bail!("[access] invite_only: M3 servers are invite-only");
        }
        Ok(())
    }
}
```
(`toml`'s errors name the key and line for unknown fields. `crate::MAX_CHARACTER_SLOTS` is 20 and lives in `lib.rs`.)

- [ ] **Step 3: `clock.rs`** (no test of its own; the account tests use it):

```rust
//! Time, injectable so tests can expire tokens without waiting.
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync {
    /// Unix milliseconds.
    fn now_ms(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
    }
}

/// A clock tests move by hand.
#[derive(Clone, Default)]
pub struct TestClock(Arc<AtomicI64>);

impl TestClock {
    pub fn at(ms: i64) -> Self {
        Self(Arc::new(AtomicI64::new(ms)))
    }

    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}
```

- [ ] **Step 4: Write the failing account-flow tests** (`accounts.rs` tests; argon2 runs at full speed under the profile override):

```rust
use std::net::{IpAddr, Ipv4Addr};

async fn setup() -> (tempfile::TempDir, Accounts, TestClock, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("f.db")).await.unwrap();
    let clock = TestClock::at(1_800_000_000_000);
    let code = InviteCode::generate().unwrap();
    store.create_invite(&code.hash(), Some("t"), clock.now_ms(), None).await.unwrap();
    let a = Accounts::new(store, AuthConfig::default(), Arc::new(clock.clone()));
    (dir, a, clock, code.display())
}

const ADDR: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3));

#[tokio::test]
async fn register_sign_in_and_get_a_single_use_game_token() {
    let (_d, a, _c, code) = setup().await;
    a.register(&code, "tester", "a long password").await.unwrap();
    let s = a.sign_in("TESTER", "a long password", ADDR).await.unwrap();
    let g = a.issue_game_token(&s.token).await.unwrap();
    assert!(g.ticket.starts_with("fenji1-"));
    assert_eq!((g.username.as_str(), g.expires_in_secs), ("tester", 60));
    assert_eq!(a.consume_game_token(&g.ticket).await.unwrap().unwrap().username, "tester");
    assert!(a.consume_game_token(&g.ticket).await.unwrap().is_none());
}

#[tokio::test]
async fn an_invalid_invite_is_reported_before_the_username() {
    let (_d, a, _c, code) = setup().await;
    a.register(&code, "taken", "a long password").await.unwrap();
    let e = a.register("0000-0000-0000-0000", "taken", "a long password").await.unwrap_err();
    assert!(matches!(e, AccountsError::InvalidInvite));
}

#[tokio::test]
async fn wrong_passwords_and_unknown_users_look_the_same() {
    let (_d, a, _c, code) = setup().await;
    a.register(&code, "tester", "a long password").await.unwrap();
    let e1 = a.sign_in("tester", "wrong password!", ADDR).await.unwrap_err();
    let e2 = a.sign_in("nobody", "wrong password!", ADDR).await.unwrap_err();
    assert_eq!(e1.to_string(), e2.to_string());
}

#[tokio::test]
async fn five_failures_lock_the_pair() {
    let (_d, a, _c, code) = setup().await;
    a.register(&code, "tester", "a long password").await.unwrap();
    for _ in 0..5 {
        let _ = a.sign_in("tester", "wrong password!", ADDR).await;
    }
    assert!(matches!(a.sign_in("tester", "a long password", ADDR).await, Err(AccountsError::TryLater(_))));
}

#[tokio::test]
async fn tokens_expire_and_sessions_end() {
    let (_d, a, c, code) = setup().await;
    a.register(&code, "tester", "a long password").await.unwrap();
    let s = a.sign_in("tester", "a long password", ADDR).await.unwrap();
    let g = a.issue_game_token(&s.token).await.unwrap();
    c.advance(60_000);
    assert!(a.consume_game_token(&g.ticket).await.unwrap().is_none());
    a.sign_out(&s.token).await.unwrap();
    assert!(matches!(a.issue_game_token(&s.token).await, Err(AccountsError::BadSession)));
}

#[tokio::test]
async fn garbage_tickets_are_simply_unknown() {
    let (_d, a, _c, _) = setup().await;
    for t in ["", "fenji-dev", "{\"auth_type\":\"mocked\"}", "fenji1-zz"] {
        assert!(a.consume_game_token(t).await.unwrap().is_none());
    }
}
```

- [ ] **Step 5: Implement `accounts.rs`; run until the tests pass.**

```rust
//! The account flows (M3 spec D3, D6, D12): registering with an invite, signing in and out, and
//! issuing and consuming game tokens. Hashing runs on the blocking pool, at most four at a time.
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fenji_auth::invite::InviteCode;
use fenji_auth::limiter::{Limiter, LimiterConfig, Verdict};
use fenji_auth::rules::{self, RuleError};
use fenji_auth::token::Token;
use fenji_auth::password;
use fenji_store::{Account, Store};
use tokio::sync::Semaphore;

use crate::clock::Clock;
use crate::config::AuthConfig;

#[derive(Debug, thiserror::Error)]
pub enum AccountsError {
    #[error("the invite is not valid")]
    InvalidInvite,
    #[error("that username is taken")]
    UsernameTaken,
    #[error("{0}")]
    Rule(RuleError),
    #[error("wrong username or password")]
    BadCredentials,
    #[error("too many attempts; try again in {} s", .0.as_secs().max(1))]
    TryLater(Duration),
    #[error("not signed in")]
    BadSession,
    #[error("internal error")]
    Internal(String),
}

impl From<fenji_store::Error> for AccountsError {
    fn from(e: fenji_store::Error) -> Self {
        match e {
            fenji_store::Error::InviteInvalid => Self::InvalidInvite,
            fenji_store::Error::UsernameTaken => Self::UsernameTaken,
            other => Self::Internal(other.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionGrant {
    pub token: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
pub struct GameGrant {
    pub ticket: String,
    pub expires_in_secs: u64,
    pub account_id: i64,
    pub username: String,
}

pub struct Accounts {
    store: Store,
    limiter: Mutex<Limiter>,
    hashing: Semaphore,
    cfg: AuthConfig,
    clock: Arc<dyn Clock>,
}

impl Accounts {
    pub fn new(store: Store, cfg: AuthConfig, clock: Arc<dyn Clock>) -> Self {
        Self { store, limiter: Mutex::new(Limiter::new(LimiterConfig::default())), hashing: Semaphore::new(4), cfg, clock }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    async fn blocking<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Result<T, AccountsError> {
        let _permit = self.hashing.acquire().await.map_err(|e| AccountsError::Internal(e.to_string()))?;
        tokio::task::spawn_blocking(f).await.map_err(|e| AccountsError::Internal(e.to_string()))
    }

    pub async fn register(&self, invite: &str, username: &str, password_: &str) -> Result<i64, AccountsError> {
        let code = InviteCode::parse(invite).ok_or(AccountsError::InvalidInvite)?;
        rules::check_username(username).map_err(AccountsError::Rule)?;
        rules::check_password(password_).map_err(AccountsError::Rule)?;
        let pw = password_.to_owned();
        let phc = self.blocking(move || password::hash(&pw)).await?.map_err(|e| AccountsError::Internal(e.to_string()))?;
        Ok(self.store.redeem_invite(&code.hash(), username, &phc, self.clock.now_ms()).await?)
    }

    pub async fn sign_in(&self, username: &str, password_: &str, addr: IpAddr) -> Result<SessionGrant, AccountsError> {
        match self.limiter.lock().expect("limiter").check(username, addr, Instant::now()) {
            Verdict::Allow => {}
            Verdict::Locked { retry_after } | Verdict::SlowDown { retry_after } => return Err(AccountsError::TryLater(retry_after)),
        }
        let account = self.store.account_by_username(username).await?;
        let pw = password_.to_owned();
        let ok = match account.clone() {
            Some(a) => self.blocking(move || password::verify(&pw, &a.password_phc)).await?.map_err(|e| AccountsError::Internal(e.to_string()))?,
            None => {
                self.blocking(move || password::verify_dummy(&pw)).await?;
                false
            }
        };
        let Some(account) = account.filter(|_| ok) else {
            self.limiter.lock().expect("limiter").record_failure(username, addr, Instant::now());
            return Err(AccountsError::BadCredentials);
        };
        self.limiter.lock().expect("limiter").record_success(username, addr);
        let now = self.clock.now_ms();
        self.store.record_sign_in(account.id, now).await?;
        let token = Token::generate().map_err(|e| AccountsError::Internal(e.to_string()))?;
        let expires_at = now + self.cfg.session_ttl_secs as i64 * 1000;
        self.store.create_session(&token.hash(), account.id, now, expires_at).await?;
        Ok(SessionGrant { token: token.as_string(), expires_at })
    }

    async fn session(&self, session: &str) -> Result<Account, AccountsError> {
        let t = Token::parse(session).ok_or(AccountsError::BadSession)?;
        let id = self.store.session_account(&t.hash(), self.clock.now_ms()).await?.ok_or(AccountsError::BadSession)?;
        self.store.account_by_id(id).await?.ok_or(AccountsError::BadSession)
    }

    pub async fn sign_out(&self, session: &str) -> Result<(), AccountsError> {
        let t = Token::parse(session).ok_or(AccountsError::BadSession)?;
        self.store.delete_session(&t.hash()).await?;
        Ok(())
    }

    pub async fn issue_game_token(&self, session: &str) -> Result<GameGrant, AccountsError> {
        let account = self.session(session).await?;
        let token = Token::generate().map_err(|e| AccountsError::Internal(e.to_string()))?;
        let ttl = self.cfg.game_token_ttl_secs;
        self.store.create_game_token(&token.hash(), account.id, self.clock.now_ms() + ttl as i64 * 1000).await?;
        Ok(GameGrant { ticket: token.as_string(), expires_in_secs: ttl, account_id: account.id, username: account.username })
    }

    /// The account a ticket names, spending it; `None` for anything that isn't a live, unspent
    /// Fenji game token.
    pub async fn consume_game_token(&self, ticket: &str) -> Result<Option<Account>, AccountsError> {
        let Some(t) = Token::parse(ticket) else { return Ok(None) };
        match self.store.consume_game_token(&t.hash(), self.clock.now_ms()).await? {
            Some(id) => Ok(self.store.account_by_id(id).await?),
            None => Ok(None),
        }
    }

    pub async fn purge(&self) -> Result<u64, AccountsError> {
        Ok(self.store.purge_expired(self.clock.now_ms()).await?)
    }
}
```

- [ ] **Step 6: Write the failing API tests** (`tests/api.rs`, driving the router in process with `tower::ServiceExt::oneshot` and `axum::extract::connect_info::MockConnectInfo`):

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode};
use fenji_auth::invite::InviteCode;
use fenji_server::accounts::Accounts;
use fenji_server::api::{AppState, ServerInfo, router};
use fenji_server::clock::TestClock;
use fenji_server::config::AuthConfig;
use fenji_store::Store;
use serde_json::{Value, json};
use tower::ServiceExt;

struct Api {
    _dir: tempfile::TempDir,
    app: axum::Router,
    invite: String,
}

async fn api_with(trusted: Vec<std::net::IpAddr>, peer: &str) -> Api {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("f.db")).await.unwrap();
    let clock = TestClock::at(1_800_000_000_000);
    let code = InviteCode::generate().unwrap();
    store.create_invite(&code.hash(), None, 1, None).await.unwrap();
    let accounts = Arc::new(Accounts::new(store, AuthConfig::default(), Arc::new(clock)));
    let info = ServerInfo { name: "Fenji".into(), version: env!("CARGO_PKG_VERSION").into(), pinned_build: "10015727".into(), gate_host: "127.0.0.1".into(), gate_port: 7800, server_id: 1 };
    let app = router(AppState { accounts, info, trusted_proxies: trusted }).layer(MockConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    Api { _dir: dir, app, invite: code.display() }
}

async fn api() -> Api {
    api_with(Vec::new(), "10.0.0.1:5000").await
}

async fn call(app: &axum::Router, method: &str, uri: &str, bearer: Option<&str>, body: Option<Value>, xff: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = bearer {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    if let Some(x) = xff {
        req = req.header("x-forwarded-for", x);
    }
    let req = match body {
        Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())).unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn register_and_sign_in(a: &Api) -> String {
    let (s, _) = call(&a.app, "POST", "/api/v1/accounts", None, Some(json!({"invite": a.invite, "username": "tester", "password": "a long password"})), None).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, v) = call(&a.app, "POST", "/api/v1/sessions", None, Some(json!({"username": "tester", "password": "a long password"})), None).await;
    assert_eq!(s, StatusCode::OK);
    v["token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_whole_flow_gives_a_game_token_with_the_gate() {
    let a = api().await;
    let token = register_and_sign_in(&a).await;
    let (s, v) = call(&a.app, "POST", "/api/v1/game-tokens", Some(&token), None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["ticket"].as_str().unwrap().starts_with("fenji1-"));
    assert_eq!(v["gate_host"], "127.0.0.1");
    assert_eq!(v["gate_port"], 7800);
    assert_eq!(v["server_id"], 1);
    assert_eq!(v["server_name"], "Fenji");
    assert_eq!(v["account"], "tester");
    assert!(v["user_id"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn bad_requests_get_stable_error_codes() {
    let a = api().await;
    let reg = |inv: &str, user: &str, pw: &str| json!({"invite": inv, "username": user, "password": pw});
    let (s, v) = call(&a.app, "POST", "/api/v1/accounts", None, Some(reg("0000-0000-0000-0000", "tester", "a long password")), None).await;
    assert_eq!((s, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_invite")));
    let (_, v) = call(&a.app, "POST", "/api/v1/accounts", None, Some(reg(&a.invite, "a:b", "a long password")), None).await;
    assert_eq!(v["error"], "bad_username");
    let (_, v) = call(&a.app, "POST", "/api/v1/accounts", None, Some(reg(&a.invite, "tester", "short")), None).await;
    assert_eq!(v["error"], "bad_password");
    let (s, v) = call(&a.app, "POST", "/api/v1/sessions", None, Some(json!({"username": "nobody", "password": "whatever123"})), None).await;
    assert_eq!((s, v["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("bad_credentials")));
    let (s, v) = call(&a.app, "POST", "/api/v1/game-tokens", Some("fenji1-00"), None, None).await;
    assert_eq!((s, v["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("bad_session")));
}

#[tokio::test]
async fn sign_out_ends_the_session() {
    let a = api().await;
    let token = register_and_sign_in(&a).await;
    let (s, _) = call(&a.app, "DELETE", "/api/v1/sessions", Some(&token), None, None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = call(&a.app, "POST", "/api/v1/game-tokens", Some(&token), None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_limiter_answers_429_with_a_retry_time() {
    let a = api().await;
    register_and_sign_in(&a).await;
    for _ in 0..5 {
        call(&a.app, "POST", "/api/v1/sessions", None, Some(json!({"username": "tester", "password": "wrong password"})), None).await;
    }
    let (s, v) = call(&a.app, "POST", "/api/v1/sessions", None, Some(json!({"username": "tester", "password": "a long password"})), None).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert!(v["retry_after_secs"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn forwarded_for_counts_only_from_a_trusted_proxy() {
    // Behind a trusted proxy, each X-Forwarded-For address is limited on its own.
    let a = api_with(vec!["127.0.0.1".parse().unwrap()], "127.0.0.1:4000").await;
    register_and_sign_in(&a).await;
    for _ in 0..5 {
        call(&a.app, "POST", "/api/v1/sessions", None, Some(json!({"username": "tester", "password": "wrong password"})), Some("203.0.113.9")).await;
    }
    let (s, _) = call(&a.app, "POST", "/api/v1/sessions", None, Some(json!({"username": "tester", "password": "a long password"})), Some("203.0.113.10")).await;
    assert_eq!(s, StatusCode::OK, "another forwarded address isn't locked");
    // From an untrusted peer, the header is ignored: all attempts count against the peer.
    let b = api().await;
    register_and_sign_in(&b).await;
    for i in 0..5 {
        call(&b.app, "POST", "/api/v1/sessions", None, Some(json!({"username": "tester", "password": "wrong password"})), Some(&format!("198.51.100.{i}"))).await;
    }
    let (s, _) = call(&b.app, "POST", "/api/v1/sessions", None, Some(json!({"username": "tester", "password": "a long password"})), Some("198.51.100.99")).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn oversized_bodies_are_refused() {
    let a = api().await;
    let big = json!({"username": "x".repeat(20_000), "password": "y"});
    let (s, _) = call(&a.app, "POST", "/api/v1/sessions", None, Some(big), None).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn server_info_names_the_build() {
    let a = api().await;
    let (s, v) = call(&a.app, "GET", "/api/v1/server", None, None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!((v["name"].as_str(), v["pinned_build"].as_str(), v["api_version"].as_u64()), (Some("Fenji"), Some("10015727"), Some(1)));
}
```

- [ ] **Step 7: Implement `api.rs`; run until the tests pass.**

| Route | Request | Success | Errors (`{"error": …}`) |
|---|---|---|---|
| `POST /api/v1/accounts` | `{invite, username, password}` | 201 `{username}` | 400 `invalid_invite`, `username_taken`, `bad_username`, `bad_password` |
| `POST /api/v1/sessions` | `{username, password}` | 200 `{token, expires_at}` | 401 `bad_credentials`; 429 `try_later` + `retry_after_secs` |
| `DELETE /api/v1/sessions` | `Authorization: Bearer <token>` | 204 | 401 `bad_session` |
| `POST /api/v1/game-tokens` | Bearer | 200 `{ticket, expires_in_secs, gate_host, gate_port, server_id, server_name, account, user_id}` | 401 `bad_session` |
| `GET /api/v1/server` | — | 200 `{name, version, pinned_build, gate_host, gate_port, api_version: 1}` | — |

Other `AccountsError` values become 500 `internal`, with the detail logged (no secret is in it). The client address is the peer's from `ConnectInfo<SocketAddr>`. Only when the peer is in `trusted_proxies` does it come from the last comma-separated entry of `X-Forwarded-For` that parses as an IP. Layers: `DefaultBodyLimit::max(16 * 1024)` and `tower_http::timeout::TimeoutLayer::new(Duration::from_secs(10))`.

```rust
#[derive(Clone)]
pub struct AppState {
    pub accounts: Arc<Accounts>,
    pub info: ServerInfo,
    pub trusted_proxies: Vec<IpAddr>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub pinned_build: String,
    pub gate_host: String,
    pub gate_port: u16,
    pub server_id: u32,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/accounts", post(register))
        .route("/api/v1/sessions", post(sign_in).delete(sign_out))
        .route("/api/v1/game-tokens", post(game_token))
        .route("/api/v1/server", get(server_info))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(TimeoutLayer::new(Duration::from_secs(10)))
        .with_state(state)
}

fn client_addr(peer: SocketAddr, headers: &HeaderMap, trusted: &[IpAddr]) -> IpAddr {
    if !trusted.contains(&peer.ip()) {
        return peer.ip();
    }
    headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|a| a.trim().parse::<IpAddr>().ok())
        .last()
        .unwrap_or(peer.ip())
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ")
}
```
Each handler maps `AccountsError` with one function, `fn fail(e: AccountsError) -> (StatusCode, Json<serde_json::Value>)`, following the table.

- [ ] **Step 8: Write the failing CLI tests** (`tests/cli.rs`):

```rust
use std::path::Path;

use fenji_server::cli::{AccountCmd, Command, InviteCmd, admin};
use fenji_server::clock::TestClock;
use fenji_server::config::Config;

fn config(dir: &Path) -> Config {
    Config::parse("[server]\nname = \"Fenji\"\npublic_host = \"play.example.net\"\n", dir).unwrap()
}

#[tokio::test]
async fn invite_create_prints_the_code_and_a_link_with_the_code_in_the_fragment() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let mut out = Vec::new();
    admin(&cfg, &Command::Invite { cmd: InviteCmd::Create { label: Some("for bob".into()), expires: None } }, &mut out, &TestClock::at(1)).await.unwrap();
    let text = String::from_utf8(out).unwrap();
    let code = text.lines().find_map(|l| l.strip_prefix("code: ")).unwrap();
    assert!(fenji_auth::invite::InviteCode::parse(code).is_some());
    assert!(text.contains(&format!("https://play.example.net/#invite={code}")));
    let mut out = Vec::new();
    admin(&cfg, &Command::Invite { cmd: InviteCmd::List }, &mut out, &TestClock::at(2)).await.unwrap();
    let list = String::from_utf8(out).unwrap();
    assert!(list.contains("for bob") && !list.contains(code), "the list never shows codes: {list}");
}

#[tokio::test]
async fn cli_writes_while_a_server_holds_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let held = fenji_store::Store::open(&cfg.store.path).await.unwrap();
    let _tx_open = held.list_invites().await.unwrap();
    let mut out = Vec::new();
    admin(&cfg, &Command::Invite { cmd: InviteCmd::Create { label: None, expires: None } }, &mut out, &TestClock::at(1)).await.unwrap();
    assert_eq!(held.list_invites().await.unwrap().len(), 1);
}

#[tokio::test]
async fn account_delete_reports_what_it_did() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let mut out = Vec::new();
    admin(&cfg, &Command::Account { cmd: AccountCmd::Delete { username: "ghost".into() } }, &mut out, &TestClock::at(1)).await.unwrap();
    assert!(String::from_utf8(out).unwrap().contains("no account"));
}
```

- [ ] **Step 9: Implement `cli.rs`, `lib.rs` and `main.rs`; run until the tests pass.** `Cli { --config PATH (default fenji.toml), command: Option<Command> }`, where `None` means `run`. `Command::{Run, Invite { cmd: InviteCmd }, Account { cmd: AccountCmd }}`, `InviteCmd::{Create { label, expires }, List, Revoke { id }}` and `AccountCmd::{List, Delete { username }}`. `--expires` takes `30m`, `12h` or `7d`. `admin` writes plain lines:
  - `invite create`: `code: XXXX-…` and `link: https://<public_host>/#invite=XXXX-…`.
  - `invite list`: `id  label  created  status`, where status is `open`, `redeemed by <name>`, `revoked` or `expired`.
  - `account list`: `id  username  created  last sign-in  characters`.
  - `account delete`: `deleted <name>` or `no account named <name>`.

  `lib.rs` holds `pub mod accounts; pub mod api; pub mod cli; pub mod clock; pub mod config;` and `pub const MAX_CHARACTER_SLOTS: u32 = 20;`. `main.rs` parses `Cli`, loads the config, sets up `tracing_subscriber`, and calls `cli::admin` for admin commands or `server::run` for `run`. In this task, `server::run` serves only the API (`axum::serve` with `into_make_service_with_connect_info::<SocketAddr>()`, graceful shutdown on Ctrl-C) and purges expired tokens at start-up and every hour; Task 6 adds the gate.

- [ ] **Step 10: Full gate (with `cargo deny check`); commit; PR; review; merge.**

```bash
git add Cargo.toml Cargo.lock crates/fenji-server
git commit -m "feat(server): add fenji-server with fenji.toml, the account API and admin commands" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: `fenji-server`: the gate and the lobby (W6)

**Files:**
- Create: `crates/fenji-server/src/server.rs`, `src/gate.rs`, `src/online.rs`, `src/lobby.rs`, `crates/fenji-server/tests/common/mod.rs`, `tests/lobby.rs`
- Modify: `crates/fenji-server/src/lib.rs`, `src/main.rs`, `crates/fenji-server/Cargo.toml` (`fenji-proto`), `Cargo.toml` (`getrandom` is already there)

**Interfaces:**
- Consumes: Tasks 4–5: `fenji_proto::{handshake::{Server, Event, AuthMessage}, client::{Client, ClientEvent}, codec::StoredCodec, hello::Variants, variant::AES128, opcode::Stc, packet::{Up, script_body}, pickle, lobby}`; `Accounts`; `Config`; `Clock`.
- Produces:
  - `fenji_server::server::{start(&Config, Arc<dyn Clock>, GateLimits) -> anyhow::Result<RunningServer>, run(Config) -> anyhow::Result<()>, GateLimits}`.
  - `RunningServer { pub login_addr: SocketAddr, pub api_addr: SocketAddr }`, with `accounts(&self) -> Arc<Accounts>` and `shutdown(self)`.
  - `fenji_server::lobby::{handle(&Store, &LobbyLimits, &Account, now: i64, id: i64, &[Value]) -> Vec<Vec<u8>>, LobbyLimits { max_characters }}`.
  - The close-reason constants in `gate`: `AUTH_FAILED`, `DUPLICATE_LOGIN` and `SERVER_FULL` (net.luac's close-reason table, lobby.md §9).

- [ ] **Step 1: The bot** (`tests/common/mod.rs`): a client on `fenji_proto::client::Client` over a real socket, which sends what the real client sends on the launcher path.

```rust
#![allow(dead_code)]
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fenji_auth::invite::InviteCode;
use fenji_proto::client::{Client, ClientEvent};
use fenji_proto::handshake::AuthMessage;
use fenji_proto::packet::script_body;
use fenji_proto::pickle::{self, Value};
use fenji_server::clock::TestClock;
use fenji_server::config::Config;
use fenji_server::server::{GateLimits, RunningServer, start};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const NOW: i64 = 1_800_000_000_000;

pub fn config(dir: &Path, extra: &str) -> Config {
    let text = format!(
        "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n{extra}"
    );
    Config::parse(&text, dir).unwrap()
}

pub async fn server(dir: &Path, extra: &str, limits: GateLimits) -> (RunningServer, TestClock) {
    let clock = TestClock::at(NOW);
    (start(&config(dir, extra), Arc::new(clock.clone()), limits).await.unwrap(), clock)
}

/// A new account with a password, through an invite.
pub async fn account(s: &RunningServer, name: &str) {
    let code = InviteCode::generate().unwrap();
    s.accounts().store().create_invite(&code.hash(), None, NOW, None).await.unwrap();
    s.accounts().register(&code.display(), name, "a long password").await.unwrap();
}

/// A fresh game token for `name`.
pub async fn ticket(s: &RunningServer, name: &str) -> String {
    let session = s.accounts().sign_in(name, "a long password", "127.0.0.1".parse().unwrap()).await.unwrap();
    s.accounts().issue_game_token(&session.token).await.unwrap().ticket
}

/// The launcher path's auth payload (payloads.md, the auth payload): ticket, 0, account, user id,
/// server id, version, "", 0, the fingerprint (zeros here), language.
pub fn payload(ticket: &str, account: &str, compressed: bool) -> Vec<u8> {
    let v = vec![
        Value::Str(ticket.as_bytes().to_vec()),
        Value::Int(0),
        Value::Str(account.as_bytes().to_vec()),
        Value::Int(1),
        Value::Int(1),
        Value::Str(b"2.1.19.42851".to_vec()),
        Value::Str(Vec::new()),
        Value::Int(0),
        Value::Str(vec![0; 20]),
        Value::Str(b"en-GB".to_vec()),
    ];
    if compressed { pickle::encode_compressed(&v).unwrap() } else { pickle::encode(&v).unwrap() }
}

pub struct Bot {
    sock: TcpStream,
    client: Client,
    events: VecDeque<ClientEvent>,
}

impl Bot {
    pub async fn connect(addr: SocketAddr, payload: Vec<u8>) -> Bot {
        let sock = TcpStream::connect(addr).await.unwrap();
        let client = Client::new(AuthMessage { dict_id: [0; 16], machine: [2, 0, 0, 0, 0, 1], payload });
        let mut bot = Bot { sock, client, events: VecDeque::new() };
        bot.pump().await;
        bot
    }

    /// Reads once (at most 5 s) and feeds the client; sends whatever it answers.
    async fn pump(&mut self) -> bool {
        let mut buf = vec![0u8; 64 * 1024];
        let n = match tokio::time::timeout(Duration::from_secs(5), self.sock.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => return false,
        };
        if n == 0 {
            return false;
        }
        let Ok((events, out)) = self.client.receive(&buf[..n]) else { return false };
        if !out.is_empty() {
            self.sock.write_all(&out).await.unwrap();
        }
        self.events.extend(events);
        true
    }

    async fn next(&mut self) -> Option<ClientEvent> {
        loop {
            if let Some(e) = self.events.pop_front() {
                if matches!(e, ClientEvent::TimeSync(_)) {
                    continue;
                }
                return Some(e);
            }
            if !self.pump().await {
                return None;
            }
        }
    }

    /// Through `ok` and `SET_NET_CONNECTED` (414).
    pub async fn expect_work(&mut self) {
        assert!(matches!(self.next().await, Some(ClientEvent::Accepted)));
        let e = self.next().await.expect("414");
        let ClientEvent::Message(m) = e else { panic!("{e:?}") };
        assert_eq!(pickle::decode(&m.body).unwrap()[0], Value::Int(414));
    }

    pub async fn expect_rejected(&mut self) -> String {
        match self.next().await {
            Some(ClientEvent::Rejected(r)) => r,
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    pub async fn expect_closed(&mut self) -> String {
        match self.next().await {
            Some(ClientEvent::Closed(r)) => r,
            other => panic!("expected a close, got {other:?}"),
        }
    }

    pub async fn send(&mut self, id: i64, args: &[Value]) {
        let out = self.client.send(0xFF, &script_body(id, args).unwrap()).unwrap();
        self.sock.write_all(&out).await.unwrap();
    }

    /// The next script message: its id and arguments.
    pub async fn recv(&mut self) -> (i64, Vec<Value>) {
        let e = self.next().await.expect("a message");
        let ClientEvent::Message(m) = e else { panic!("{e:?}") };
        let mut v = pickle::decode(&m.body).unwrap();
        let Value::Int(id) = v.remove(0) else { panic!("id") };
        (id, v)
    }

    /// Nothing arrives within `ms`.
    pub async fn quiet(&mut self, ms: u64) {
        let got = tokio::time::timeout(Duration::from_millis(ms), self.next()).await;
        assert!(got.is_err() || got.unwrap().is_none(), "expected silence");
    }
}
```
(`ClientEvent::Rejected` and `Closed` carry the reason as a `String`: check `client.rs` and adapt if they carry bytes.)

- [ ] **Step 2: Write the failing lobby tests** (`tests/lobby.rs`). `request(name, vocation, skeleton)` builds the create table as `fenji-proto`'s `tests/lobby.rs` does; copy the function into `common`.

```rust
mod common;
use common::*;
use fenji_proto::lobby::{self, code};
use fenji_proto::pickle::{Table, Value};
use fenji_server::server::GateLimits;

#[tokio::test]
async fn login_list_create_restart_list_delete() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&ticket(&s, "tester").await, "tester", false)).await;
    bot.expect_work().await;
    bot.send(lobby::SERVER_LIST, &[]).await;
    bot.send(lobby::HOTPATCH_INIT, &[Value::Int(0)]).await;
    bot.send(lobby::GM_SCRIPTS, &[]).await;
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    let (id, args) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_LIST);
    assert_eq!(args[1], Value::Table(Table::default()));
    bot.send(lobby::CREATE_ACTOR, &request("Lin", 0, 2)).await;
    let (id, args) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_CREATE);
    let Value::Table(actor) = &args[0] else { panic!() };
    let pid = actor.hash.iter().find(|(k, _)| *k == Value::Str(b"pid".to_vec())).map(|(_, v)| v.clone()).unwrap();
    s.shutdown().await;

    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let mut bot = Bot::connect(s.login_addr, payload(&ticket(&s, "tester").await, "tester", true)).await;
    bot.expect_work().await;
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    let (_, args) = bot.recv().await;
    let Value::Table(list) = &args[1] else { panic!() };
    assert_eq!(list.array.len(), 1, "the character survived the restart");
    bot.send(lobby::DELETE_ACTOR, &[pid.clone()]).await;
    let (id, args) = bot.recv().await;
    assert_eq!((id, &args[0]), (lobby::ROLE_DELETE, &pid));
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    let (_, args) = bot.recv().await;
    assert_eq!(args[1], Value::Table(Table::default()));
}

#[tokio::test]
async fn a_ticket_works_once_and_must_match_its_account() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    account(&s, "other").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "TESTER", false)).await;
    bot.expect_work().await; // case doesn't matter
    let mut again = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    assert_eq!(again.expect_rejected().await, fenji_server::gate::AUTH_FAILED);
    let mut wrong = Bot::connect(s.login_addr, payload(&ticket(&s, "tester").await, "other", false)).await;
    assert_eq!(wrong.expect_rejected().await, fenji_server::gate::AUTH_FAILED);
}

#[tokio::test]
async fn expired_mocked_and_garbage_tickets_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (s, clock) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    clock.advance(60_000);
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    assert_eq!(bot.expect_rejected().await, fenji_server::gate::AUTH_FAILED);
    // The mocked SDK's ticket form (payloads.md P.6), in a default build.
    let mocked = "7b226163636f756e745f646973706c61795f6e616d65223a226d6f636b6564227d";
    let mut bot = Bot::connect(s.login_addr, payload(mocked, "account-name", false)).await;
    assert_eq!(bot.expect_rejected().await, fenji_server::gate::AUTH_FAILED);
    let mut bot = Bot::connect(s.login_addr, vec![0xFF, 0, 0, 9, 1, 2, 3]).await;
    assert_eq!(bot.expect_rejected().await, fenji_server::gate::AUTH_FAILED);
}

#[tokio::test]
async fn refusals_come_back_as_270() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "[lobby]\nmax_characters = 1\n", GateLimits::default()).await;
    account(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&ticket(&s, "tester").await, "tester", false)).await;
    bot.expect_work().await;
    bot.send(lobby::CREATE_ACTOR, &request("", 0, 2)).await;
    assert_eq!(bot.recv().await, (lobby::ERROR_CODE, vec![Value::Int(code::ROLE_NAME_LENERR)]));
    bot.send(lobby::CREATE_ACTOR, &request("Lin", 0, 2)).await;
    assert_eq!(bot.recv().await.0, lobby::ROLE_CREATE);
    bot.send(lobby::CREATE_ACTOR, &request("Two", 0, 2)).await;
    assert_eq!(bot.recv().await, (lobby::ERROR_CODE, vec![Value::Int(code::ROLE_MAX_NUMBER)]));
    bot.send(lobby::DELETE_ACTOR, &[Value::Int(999_999)]).await;
    assert_eq!(bot.recv().await, (lobby::ERROR_CODE, vec![Value::Int(code::ROLE_NOT_FOUND)]));
    bot.send(lobby::SELECT_ACTOR, &[Value::Int(1)]).await;
    assert_eq!(bot.recv().await, (lobby::ERROR_CODE, vec![Value::Int(code::SERVER_NOT_RUNNING)]));
    bot.send(lobby::CHANGE_SLOT, &[Value::Int(1), Value::Int(1)]).await; // ignored
    bot.send(12_345, &[]).await; // unknown ids are ignored, and the connection stays
    bot.quiet(300).await;
}

#[tokio::test]
async fn a_taken_name_is_refused_for_everyone() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "aaa").await;
    account(&s, "bbb").await;
    let mut a = Bot::connect(s.login_addr, payload(&ticket(&s, "aaa").await, "aaa", false)).await;
    a.expect_work().await;
    a.send(lobby::CREATE_ACTOR, &request("Lin", 0, 2)).await;
    assert_eq!(a.recv().await.0, lobby::ROLE_CREATE);
    let mut b = Bot::connect(s.login_addr, payload(&ticket(&s, "bbb").await, "bbb", false)).await;
    b.expect_work().await;
    b.send(lobby::CREATE_ACTOR, &request("LIN", 0, 2)).await;
    assert_eq!(b.recv().await, (lobby::ERROR_CODE, vec![Value::Int(code::ROLE_NAME_EXIST)]));
}

#[tokio::test]
async fn oversized_appearance_is_refused_and_server_survives() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&ticket(&s, "tester").await, "tester", false)).await;
    bot.expect_work().await;
    let mut req = request("Lin", 0, 2);
    if let Value::Table(t) = &mut req[0] {
        t.hash[5].1 = Value::Str(vec![b'x'; 100_000]); // the face string
    }
    bot.send(lobby::CREATE_ACTOR, &req).await;
    assert_eq!(bot.recv().await.0, lobby::ERROR_CODE);
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    assert_eq!(bot.recv().await.0, lobby::ROLE_LIST);
}

#[tokio::test]
async fn the_player_cap_and_a_second_login() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits { max_players: 1, ..GateLimits::default() }).await;
    account(&s, "aaa").await;
    account(&s, "bbb").await;
    let mut a = Bot::connect(s.login_addr, payload(&ticket(&s, "aaa").await, "aaa", false)).await;
    a.expect_work().await;
    let mut b = Bot::connect(s.login_addr, payload(&ticket(&s, "bbb").await, "bbb", false)).await;
    assert_eq!(b.expect_rejected().await, fenji_server::gate::SERVER_FULL);
    let mut a2 = Bot::connect(s.login_addr, payload(&ticket(&s, "aaa").await, "aaa", false)).await;
    a2.expect_work().await;
    assert_eq!(a.expect_closed().await, fenji_server::gate::DUPLICATE_LOGIN);
}

#[tokio::test]
async fn silent_connection_is_closed_after_the_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits { auth_deadline: std::time::Duration::from_millis(300), ..GateLimits::default() }).await;
    let mut sock = tokio::net::TcpStream::connect(s.login_addr).await.unwrap();
    let mut buf = vec![0u8; 256];
    use tokio::io::AsyncReadExt;
    let mut got = 0;
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(3), sock.read(&mut buf)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => got += n,
            Ok(Err(_)) => break,
            Err(_) => panic!("not closed after the deadline"),
        }
    }
    assert_eq!(got, 64, "only the hello");
}

#[tokio::test]
async fn oversized_pre_auth_input_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let mut bot = Bot::connect(s.login_addr, vec![b'a'; 0x5000]).await;
    let mut closed = false;
    for _ in 0..3 {
        if bot.next_is_end().await {
            closed = true;
            break;
        }
    }
    assert!(closed);
}
```
`Bot::next_is_end(&mut self) -> bool` is true when the socket ends or the client closes. Add it to `common`: `matches!(self.next().await, None | Some(ClientEvent::Rejected(_) | ClientEvent::Closed(_)))`.

- [ ] **Step 3: Run (`cargo test -p fenji-server --test lobby`); expect FAIL (no `server` module).**

- [ ] **Step 4: Implement `online.rs`** (the online-accounts registry, D17):

```rust
//! Who is in `WORK` (M3 spec D17): at most `max_players` accounts, and one connection per
//! account; a second login replaces the first, which is told to close.
use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::oneshot;

pub struct Online {
    max: usize,
    next: Mutex<u64>,
    accounts: Mutex<HashMap<i64, (u64, oneshot::Sender<()>)>>,
}

pub enum Join {
    /// The connection's id (to leave with) and the receiver that fires when a newer login replaces it.
    Joined(u64, oneshot::Receiver<()>),
    Full,
}

impl Online {
    pub fn new(max: usize) -> Self {
        Self { max, next: Mutex::new(0), accounts: Mutex::new(HashMap::new()) }
    }

    pub fn join(&self, account: i64) -> Join {
        let mut map = self.accounts.lock().expect("online");
        if !map.contains_key(&account) && map.len() >= self.max {
            return Join::Full;
        }
        let id = {
            let mut n = self.next.lock().expect("online");
            *n += 1;
            *n
        };
        let (tx, rx) = oneshot::channel();
        if let Some((_, old)) = map.insert(account, (id, tx)) {
            let _ = old.send(());
        }
        Join::Joined(id, rx)
    }

    /// Leaves only if `id` is still the account's connection.
    pub fn leave(&self, account: i64, id: u64) {
        let mut map = self.accounts.lock().expect("online");
        if map.get(&account).is_some_and(|(cur, _)| *cur == id) {
            map.remove(&account);
        }
    }
}
```

- [ ] **Step 5: Implement `lobby.rs`** (server side; one function per message, with the store calls inside):

```rust
//! The lobby (M3 spec D8–D11; docs/protocol/packets/lobby.md). It lists, creates and deletes
//! against the store, refuses `SelectActor` until M4, and ignores the rest: 402, 370, 496, 321 and
//! 392 need no answer. Every 292 gets a 228, every 304 a 628 or a 270, and every 166 an 84 or a 270,
//! because the client's screens wait for them (lobby.md §6–§7). It never sends `lobby::NEVER_SEND`.
use fenji_proto::lobby::{self, AccountExtras, Actor, Appearance, CreateRequest, code};
use fenji_proto::pickle::{self, Table, Value};
use fenji_store::{Account, Character, NewCharacter, Store};

pub struct LobbyLimits {
    pub max_characters: u32,
}

impl LobbyLimits {
    fn extras(&self) -> AccountExtras {
        AccountExtras { role_num: i64::from(self.max_characters.saturating_sub(lobby::ROLE_SLOTS)) }
    }
}

fn actor(c: &Character) -> Actor {
    let appearance = match pickle::decode(&c.appearance).ok().and_then(|mut v| v.pop()) {
        Some(Value::Table(t)) => Appearance::from_table(&t),
        _ => Appearance::from_table(&Table::default()),
    };
    Actor { pid: c.pid, name: c.name.clone(), level: c.level, vocation: c.vocation, skeleton: c.skeleton, appearance }
}

pub async fn handle(store: &Store, limits: &LobbyLimits, account: &Account, now: i64, id: i64, args: &[Value]) -> Vec<Vec<u8>> {
    let reply = match id {
        lobby::QUERY_ACTORS => list(store, limits, account).await,
        lobby::CREATE_ACTOR => create(store, limits, account, now, args).await,
        lobby::DELETE_ACTOR => delete(store, account, args).await,
        lobby::SELECT_ACTOR => Ok(vec![lobby::error_code(code::SERVER_NOT_RUNNING)]),
        other => {
            tracing::debug!(id = other, "script message ignored");
            Ok(Vec::new())
        }
    };
    reply.unwrap_or_else(|e| {
        // The screens wait for an answer: give one, and log the cause.
        tracing::error!(error = %e, id, "lobby store error");
        vec![lobby::error_code(code::SERVER_NOT_RESP)]
    })
}

async fn list(store: &Store, limits: &LobbyLimits, account: &Account) -> anyhow::Result<Vec<Vec<u8>>> {
    let actors: Vec<Actor> = store.list_characters(account.id).await?.iter().map(actor).collect();
    Ok(vec![lobby::role_list(0, &actors, &limits.extras())?])
}

async fn create(store: &Store, limits: &LobbyLimits, account: &Account, now: i64, args: &[Value]) -> anyhow::Result<Vec<Vec<u8>>> {
    let req = match CreateRequest::parse(args) {
        Ok(r) => r,
        Err(refusal) => return Ok(vec![lobby::error_code(refusal.code())]),
    };
    let new = NewCharacter {
        name: req.name,
        vocation: req.vocation,
        skeleton: req.skeleton,
        appearance: pickle::encode(&[Value::Table(req.appearance.to_table())])?,
    };
    match store.create_character(account.id, &new, limits.max_characters, now).await {
        Ok(c) => {
            let slot = store.list_characters(account.id).await?.len() as i64;
            Ok(vec![lobby::create_result(&actor(&c), slot)?])
        }
        Err(fenji_store::Error::NameTaken) => Ok(vec![lobby::error_code(code::ROLE_NAME_EXIST)]),
        Err(fenji_store::Error::TooMany) => Ok(vec![lobby::error_code(code::ROLE_MAX_NUMBER)]),
        Err(e) => Err(e.into()),
    }
}

async fn delete(store: &Store, account: &Account, args: &[Value]) -> anyhow::Result<Vec<Vec<u8>>> {
    let Some(Value::Int(pid)) = args.first() else {
        return Ok(vec![lobby::error_code(code::ROLE_NOT_FOUND)]);
    };
    Ok(vec![if store.delete_character(account.id, *pid).await? { lobby::delete_result(*pid) } else { lobby::error_code(code::ROLE_NOT_FOUND) }])
}
```

- [ ] **Step 6: Implement `gate.rs`.** One task per connection, in this order; constants from lobby.md §9:

```rust
//! The gate listener (M3 spec D3, D17; docs/protocol/transport.md): the hello, the auth with its
//! deadline and size cap, the ticket, the online registry, then the lobby loop. Reasons for
//! closing use the client's own keys (docs/protocol/packets/lobby.md §9). The ticket field is
//! never logged.
pub const AUTH_FAILED: &str = "error auth-failed";
pub const DUPLICATE_LOGIN: &str = "error duplicate-login";
/// The client's close-reason table (net.luac) has no key for a full server, so this shows as
/// "Internal server error: server full" (run m2-r3b showed that form).
pub const SERVER_FULL: &str = "server full";
/// The most bytes taken before the auth message is complete: its 26-byte head (with the inner
/// header) and a payload of at most 0x4000 bytes (transport.md 1.1, §6), plus frame lengths and
/// a few time syncs.
pub const PRE_AUTH_CAP: usize = 26 + 4 + 0x4000 + 512;
/// No frame from the client for this long ends the connection (it sends a time sync every 2 s).
pub const IDLE: std::time::Duration = std::time::Duration::from_secs(60);
```

`async fn connection(sock: TcpStream, ctx: Arc<GateCtx>)`:
1. `ctx.unauthenticated.try_acquire_owned()`. With no permit (64 are in use), drop the socket.
2. `Server::start(fill, Variants { down: AES128, up: AES128 })`, where `fill` is 64 bytes from `getrandom::fill`; write the hello.
3. Under `tokio::time::timeout(limits.auth_deadline, …)`, read and feed `server.receive` until an `Event::Auth` arrives, counting the bytes read. Past `PRE_AUTH_CAP`, or on a receive error, timeout or EOF, drop the socket.
4. Release the permit. `pickle::decode(&auth.payload)`. Take value 0 as the ticket and value 2 as the account when both are strings; otherwise reject with `AUTH_FAILED`. `ctx.accounts.consume_game_token(ticket)`. `None`, or an account whose username differs from value 2 ignoring ASCII case, rejects with `AUTH_FAILED` (`server.reject(reason, &StoredCodec)`, written, then close).
5. `ctx.online.join(account.id)`. `Full` rejects with `SERVER_FULL`.
6. Write `server.accept(&StoredCodec)` then `server.begin_work(&StoredCodec)`. Log `account = username, peer` at info.
7. The loop, with `tokio::select!` over:
   - a socket read under `tokio::time::timeout(IDLE, …)`. A timeout or EOF ends the connection. Bytes go to `server.receive`, whose `Event::Message` goes to `Up::decode`. A `Script { id: Value::Int(id), args, .. }` goes to `lobby::handle`, and each reply goes out through `server.send(Stc::Script.to_u8(), &reply, &StoredCodec)`. Undecodable messages are logged (type and length only) and skipped. Time syncs are ignored.
   - the replacement receiver: send a type-1 close (`server.send(Stc::Close.to_u8(), DUPLICATE_LOGIN.as_bytes(), &StoredCodec)`), flush, end.
8. Always `ctx.online.leave(account.id, conn_id)`.

`GateLimits { max_players: usize, auth_deadline: Duration, max_unauthenticated: usize }`, with `Default` 20, 30 s and 64. `GateLimits::from_config(&Config)` takes `max_players` from `[server]`. `server::start` takes `GateLimits` whole: `run` builds it with `from_config`, and tests pass their own.

- [ ] **Step 7: Implement `server.rs`.** `start` does the following:
  - opens the store and builds `Accounts`;
  - binds `login_bind` and `api_bind` (port 0 allowed);
  - spawns the gate accept loop, with the connections in a `JoinSet` owned by that task, so aborting the task ends them;
  - spawns `axum::serve` with connect-info;
  - spawns the hourly purge;
  - returns `RunningServer`, which holds the handles, the store and the actual addresses.

  `shutdown` aborts the tasks, awaits them, then `store.close()`. `run(cfg)` starts the server, logs both addresses, waits for Ctrl-C or SIGTERM, then shuts down. `main.rs`'s `run` calls it.

- [ ] **Step 8: Run `cargo test -p fenji-server`; expect PASS. Then the full gate.** The restart test must pass several times in a row (`cargo test -p fenji-server --test lobby -- --test-threads 1` ×3), so no open file or socket leaks across `shutdown`.

- [ ] **Step 9: Commit; PR; review; merge.**

```bash
git add Cargo.toml Cargo.lock crates/fenji-server
git commit -m "feat(server): add the gate and the lobby: tickets, limits, list, create and delete" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: `fenji-launcher` and `fenji-launch` (W7)

**Files:**
- Create: `crates/fenji-launcher/Cargo.toml`, `src/lib.rs`, `src/api.rs`, `src/manifest.rs`, `src/verify.rs`, `src/launch.rs`, `src/login_path.rs`, `src/invite_link.rs`, `src/settings.rs`, `src/bin/fenji-launch.rs`, `crates/fenji-launcher/manifests/10015727.tsv`, `crates/fenji-server/tests/launcher.rs`, `crates/fenji-launcher/tests/cli.rs`
- Modify: `Cargo.toml` (member; `reqwest = { version = "0.13.5", default-features = false, features = ["rustls-no-provider", "json", "http2"] }`, `rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12", "logging"] }`, `fenji-launcher = { path = "crates/fenji-launcher" }`), `crates/fenji-server/Cargo.toml` (dev-dependency `fenji-launcher`), `deny.toml` (`[graph] targets`, D20), `Cargo.lock`

**Interfaces:**
- Consumes: Task 5's API (routes and JSON); `fenji_auth::invite::InviteCode`.
- Produces:
  - `fenji_launcher::api::{ApiClient, ApiError, ServerInfo, Session, GameGrant}`. `GameGrant` carries `ticket`, `expires_in_secs`, `gate_host`, `gate_port`, `server_id`, `server_name`, `account` and `user_id`.
  - `fenji_launcher::manifest::{Manifest, Entry}`, with `parse`, `pinned`, `generate` and `to_tsv`.
  - `fenji_launcher::verify::{verify, Level, Report}`.
  - `fenji_launcher::launch::{LaunchSpec, Platform, base_spec, resolve_ipv4, LaunchError}`.
  - `fenji_launcher::login_path::{LoginPath, available, GATE_NOTICE}`, plus `Uinfo` under `feature = "dev-login-path"`.
  - `fenji_launcher::invite_link::parse`.
  - `fenji_launcher::settings::{Settings, config_dir}`.
  - The binary `fenji-launch`.

- [ ] **Step 1: Scaffold and the D20 licence change.** In `crates/fenji-launcher/Cargo.toml`:
  - `[features] dev-login-path = []`;
  - dependencies: `fenji-auth`, `reqwest`, `rustls`, `serde`, `serde_json`, `sha2`, `thiserror`, `tokio`, `clap`, `anyhow`;
  - dev-dependencies: `tempfile`.

  In the root `deny.toml`:
```toml
[graph]
all-features = true
# The platforms Fenji builds on: Linux and Windows for players and hosts, macOS for development
# (M3 spec D20). Target-only dependencies elsewhere (such as wasm's webpki-root-certs) don't count.
targets = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc", "x86_64-pc-windows-gnu", "aarch64-apple-darwin"]
```
Run `cargo deny check`; expect it to pass with no new licence.

- [ ] **Step 2: Write the failing manifest and verify tests.**

```rust
// manifest.rs tests
#[test]
fn tsv_round_trips() {
    let m = Manifest { entries: vec![Entry { path: "bin64/a.dll".into(), size: 3, sha256: [1; 32] }] };
    assert_eq!(Manifest::parse(&m.to_tsv()).unwrap(), m);
}

#[test]
fn the_pinned_manifest_is_the_pinned_build() {
    let m = Manifest::pinned();
    assert_eq!(m.entries.len(), 210);
    assert_eq!(m.entries.iter().map(|e| e.size).sum::<u64>(), 107_996_004_001);
    let exe = m.entries.iter().find(|e| e.path == "bin64/gujianol.exe").unwrap();
    assert_eq!(hex(&exe.sha256), "3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678");
    assert_eq!(exe.size, 54_029_984);
}

#[test]
fn parse_refuses_damaged_lines() {
    assert!(Manifest::parse("bin64/a.dll\t3\n").is_err());
    assert!(Manifest::parse("bin64/a.dll\tx\t00\n").is_err());
    assert!(Manifest::parse("../a\t3\t".to_string().as_str()).is_err());
}

// verify.rs tests
fn tree() -> (tempfile::TempDir, Manifest) {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("bin64")).unwrap();
    std::fs::create_dir_all(d.path().join("data")).unwrap();
    std::fs::write(d.path().join("bin64/gujianol.exe"), b"exe!").unwrap();
    std::fs::write(d.path().join("data/data000"), b"archive").unwrap();
    let m = Manifest::generate(d.path()).unwrap();
    (d, m)
}

#[test]
fn an_intact_tree_passes_both_levels() {
    let (d, m) = tree();
    assert!(verify(d.path(), &m, Level::Quick).unwrap().ok());
    assert!(verify(d.path(), &m, Level::Full).unwrap().ok());
}

#[test]
fn quick_catches_sizes_and_executables_full_catches_the_rest() {
    let (d, m) = tree();
    std::fs::write(d.path().join("bin64/gujianol.exe"), b"exe?").unwrap(); // same size
    let r = verify(d.path(), &m, Level::Quick).unwrap();
    assert_eq!(r.wrong_hash, vec!["bin64/gujianol.exe".to_string()]);
    let (d, m) = tree();
    std::fs::write(d.path().join("data/data000"), b"ARCHIVE").unwrap(); // same size
    assert!(verify(d.path(), &m, Level::Quick).unwrap().ok());
    assert_eq!(verify(d.path(), &m, Level::Full).unwrap().wrong_hash, vec!["data/data000".to_string()]);
    std::fs::remove_file(d.path().join("data/data000")).unwrap();
    assert_eq!(verify(d.path(), &m, Level::Quick).unwrap().missing, vec!["data/data000".to_string()]);
}
```

- [ ] **Step 3: Implement `manifest.rs` and `verify.rs`.** The manifest is lines `path\tsize\tsha256-hex`, sorted by path, with `#` comments. A path must be relative, use `/` and have no `..` component. `generate` walks the tree, skips nothing, and hashes files on `std::thread::available_parallelism()` scoped threads, which pull indices from an `AtomicUsize`. `pinned()` is `Manifest::parse(include_str!("../manifests/10015727.tsv")).expect("the committed manifest")`. `verify` fills `Report { missing, wrong_size, wrong_hash }`:
  - **Quick** compares every size, and hashes files under `bin64/` ending in `.exe` or `.dll`.
  - **Full** hashes everything, in parallel as `generate` does.

  `Report::ok()` is true when all three lists are empty.

- [ ] **Step 4: Generate the manifest from the pinned copy** (owner's machine, about two minutes):

```bash
sha256sum "$FENJI_CLIENT_PATH/bin64/gujianol.exe"   # must print 3fdc6394…0678 (docs/re/pinned-build.md)
cargo run --release -p fenji-launcher --bin fenji-launch -- manifest --client "$FENJI_CLIENT_PATH" --out crates/fenji-launcher/manifests/10015727.tsv
wc -l crates/fenji-launcher/manifests/10015727.tsv  # 210 entries plus the header comment
```
The header comment says what the file is: "Fenji: the pinned SoLO client (Steam build 10015727): path, size and SHA-256 of each file; hashes only, no game content (M3 spec D15)". Run the tests; expect PASS.

- [ ] **Step 5: Write the failing launch, login-path, invite-link and URL-policy tests.**

```rust
// launch.rs tests
#[test]
fn linux_runs_umu_in_its_own_prefix_from_bin64() {
    let spec = base_spec(Path::new("/games/solo"), &Platform::Linux { umu_run: "/usr/bin/umu-run".into(), prefix: "/home/p/.local/share/fenji/prefix".into(), proton: None });
    assert_eq!(spec.program, Path::new("/usr/bin/umu-run"));
    assert_eq!(spec.args, vec!["/games/solo/bin64/gujianol.exe".to_string(), "--fs=data:_index:_index".to_string()]);
    assert!(spec.env.contains(&("WINEPREFIX".into(), "/home/p/.local/share/fenji/prefix".into())));
    assert!(spec.env.contains(&("GAMEID".into(), "umu-1418100".into())));
    assert_eq!(spec.cwd, Path::new("/games/solo/bin64"));
    assert!(spec.args.iter().all(|a| !a.starts_with("--gf-sdk")), "never an SDK flag (D1)");
}

#[test]
fn windows_runs_the_exe_directly() {
    let spec = base_spec(Path::new("C:/Games/SoLO"), &Platform::Windows);
    assert_eq!(spec.program, Path::new("C:/Games/SoLO/bin64/gujianol.exe"));
    assert_eq!(spec.args, vec!["--fs=data:_index:_index".to_string()]);
}

#[test]
fn gates_resolve_to_ipv4() {
    assert_eq!(resolve_ipv4("127.0.0.1", 7800).unwrap().to_string(), "127.0.0.1:7800");
}

#[test]
fn gate_without_ipv4_is_a_clear_error() {
    let e = resolve_ipv4("no-such-host.invalid", 7800).unwrap_err().to_string();
    assert!(e.contains("no IPv4 address"), "{e}");
}

// login_path.rs tests
#[cfg(not(feature = "dev-login-path"))]
#[test]
fn a_default_build_offers_no_login_path() {
    assert!(available().is_empty());
    assert!(GATE_NOTICE.contains("#114"));
}

#[cfg(feature = "dev-login-path")]
#[test]
fn uinfo_puts_everything_in_the_environment() {
    let grant = GameGrant { ticket: "fenji1-ab".into(), expires_in_secs: 60, gate_host: "play.example.net".into(), gate_port: 7800, server_id: 1, server_name: "Fenji".into(), account: "tester".into(), user_id: 5 };
    let mut spec = LaunchSpec { program: "x".into(), args: vec![], env: vec![], cwd: "/".into() };
    Uinfo.apply(&grant, "10.0.0.2:7800".parse().unwrap(), &mut spec);
    assert_eq!(spec.env, vec![
        ("gj_uinfo".to_string(), "1:90:Fenji:0:tester:5".to_string()),
        ("gj_gates".to_string(), "10.0.0.2:7800".to_string()),
        ("gj_ticket".to_string(), "fenji1-ab".to_string()),
    ]);
    assert!(spec.args.is_empty(), "the ticket stays out of the command line (S17)");
}

// invite_link.rs tests
#[test]
fn links_and_bare_codes() {
    let (server, code) = parse("https://play.example.net/#invite=ABCD-EFGH-JKMN-PQRS").unwrap();
    assert_eq!(server.as_deref(), Some("https://play.example.net"));
    assert_eq!(code.display(), "ABCD-EFGH-JKMN-PQRS");
    let (server, _) = parse("abcd efgh jkmn pqrs").unwrap();
    assert!(server.is_none());
    assert!(parse("https://play.example.net/#nothing").is_none());
}

// api.rs tests
#[test]
fn refuses_plain_http_off_loopback() {
    assert!(ApiClient::new("http://127.0.0.1:7801").is_ok());
    assert!(ApiClient::new("http://localhost:7801").is_ok());
    assert!(ApiClient::new("http://[::1]:7801").is_ok());
    assert!(matches!(ApiClient::new("http://play.example.net"), Err(ApiError::Insecure)));
    assert!(ApiClient::new("https://play.example.net").is_ok());
    assert!(ApiClient::new("ftp://play.example.net").is_err());
}
```

- [ ] **Step 6: Implement them.**
  - **`launch.rs`** builds `LaunchSpec { program, args, env: Vec<(String, String)>, cwd }`:
    - Linux: `umu-run` with the exe and `--fs=data:_index:_index`, and `WINEPREFIX`, `GAMEID=umu-1418100` and `PROTONPATH` (when set) in the environment.
    - Windows: the exe, with the `--fs` argument.

    `resolve_ipv4` uses `ToSocketAddrs` and takes the first IPv4 address. Otherwise it returns `LaunchError::NoIpv4(host)`, displayed as "`<host>` has no IPv4 address; the game client can only connect to IPv4 gates".
  - **`login_path.rs`** holds:
    - the trait `LoginPath { fn name(&self) -> &'static str; fn apply(&self, grant: &GameGrant, gate: SocketAddrV4, spec: &mut LaunchSpec); }`;
    - `Uinfo` under the feature, which pushes the three environment pairs in the order the test shows;
    - `available()`;
    - `GATE_NOTICE`: "Starting the game from the launcher waits on the owner's decision about the login path (issue #114). Until then, developers start the client only in the sandbox (scripts/spike/run-client.sh)."
  - **`invite_link.rs`** accepts `<https-or-http-url>/#invite=<code>`, or a bare code, through `InviteCode::parse`.
  - **`api.rs`**: `ApiClient::new` parses the URL. The scheme must be `https`, or `http` with a loopback host (`127.0.0.0/8`, `::1`, `localhost`); otherwise `ApiError::Insecure`. It installs `rustls::crypto::ring::default_provider()` once (`OnceLock`, ignoring "already installed") and builds a `reqwest::Client` with a 15 s timeout. The methods follow Task 5's routes. An error body's `error` string becomes `ApiError::Server { status, code }`, and `try_later` keeps `retry_after_secs`. No method logs its arguments.
  - **`settings.rs`**: `config_dir()` is `$XDG_CONFIG_HOME/fenji-launcher`, or `$HOME/.config/fenji-launcher` (Linux); `$HOME/Library/Application Support/fenji-launcher` (macOS); `%APPDATA%\fenji-launcher` (Windows). No `dirs` crate (D20). `Settings { server: Option<String>, username: Option<String>, client: Option<PathBuf> }` is JSON, written through a temporary file and a rename.

- [ ] **Step 7: The CLI** (`src/bin/fenji-launch.rs`, clap):
  - **`redeem --server URL --invite CODE_OR_LINK --username U`**: the password comes from `--password-file F` or `FENJI_PASSWORD`, never an argument. Prints `registered <U> on <server>`.
  - **`verify --client DIR [--full]`**: prints the report; exit 1 on differences.
  - **`manifest --client DIR --out FILE`**.
  - **`play --server URL --username U [--password-file F] --print`**: sign in, game token, resolve the gate, `base_spec` for the host platform, the first available login path. It prints `KEY=VALUE` for each of the spec's `gj_*` environment pairs, one per line, and nothing else on stdout. Without the feature, it prints `GATE_NOTICE` to stderr and exits 2. Without `--print`, it explains that M3 has no `--exec` (D2) and exits 2.

  Tests (`tests/cli.rs`, through `env!("CARGO_BIN_EXE_fenji-launch")`): `play` in a default build exits 2 and prints the notice; `verify` on a temporary tree whose manifest doesn't match exits 1.

- [ ] **Step 8: The API client against a real server** (`crates/fenji-server/tests/launcher.rs`):

```rust
mod common;
use common::*;
use fenji_launcher::api::ApiClient;
use fenji_server::server::GateLimits;

#[tokio::test]
async fn the_launcher_registers_signs_in_and_gets_a_ticket_the_gate_takes() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let code = fenji_auth::invite::InviteCode::generate().unwrap();
    s.accounts().store().create_invite(&code.hash(), None, NOW, None).await.unwrap();
    let api = ApiClient::new(&format!("http://{}", s.api_addr)).unwrap();
    api.register(&code.display(), "tester", "a long password").await.unwrap();
    let session = api.sign_in("tester", "a long password").await.unwrap();
    let grant = api.game_token(&session).await.unwrap();
    assert_eq!((grant.account.as_str(), grant.server_name.as_str()), ("tester", "Fenji"));
    let mut bot = Bot::connect(s.login_addr, payload(&grant.ticket, &grant.account, true)).await;
    bot.expect_work().await;
    api.sign_out(session).await.unwrap();
}
```

- [ ] **Step 9: Full gate (`cargo deny check` included, and `cargo test -p fenji-launcher --features dev-login-path`); commit; PR; review; merge.**

```bash
git add Cargo.toml Cargo.lock deny.toml crates/fenji-launcher crates/fenji-server/Cargo.toml crates/fenji-server/tests/launcher.rs
git commit -m "feat(launcher): add fenji-launcher and fenji-launch: sign-in, verification, the launch spec" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: The runner: server mode, launcher hook, keys (W8)

**Files:**
- Create: `scripts/spike/xkey.py`
- Modify: `scripts/spike/run-client.sh`, `scripts/spike/test-run-client.sh`, `docs/re/README.md` ("Running the client")

**Interfaces:**
- Consumes: `target/release/fenji-server` (Task 6) and `target/release/fenji-launch` built with `--features dev-login-path` (Task 7).
- Produces: `run-client.sh run NAME [--server CONFIG] [--server-data DIR] [--launch "ARGS"] [--key T:KEY]... [--type T:TEXT]... …`.

- [ ] **Step 1: Read `run-client.sh`'s `cmd_run` and its sandbox script whole** before changing them. The stub's start, the readiness check (exit 7) and the evidence collection are the parts to extend. Note how `--click` is scheduled; `--key` and `--type` follow the same pattern.

- [ ] **Step 2: Write the failing tests** in `test-run-client.sh`'s style, run with no sandbox:
  - `--launch` without `--server` exits 2;
  - `--server` with an unreadable file exits 2;
  - `--server-data` outside `re/runs/` exits 2;
  - `--key 5:` and `--type x` (no time) exit 2;
  - `redact_gj_ticket` (a new function) turns `gj_ticket=fenji1-abc gj_gates=1.2.3.4:7800` into `gj_ticket=<redacted> gj_gates=1.2.3.4:7800`;
  - `parse_launch_output` keeps only `gj_[a-z_]+=…` lines and refuses a value with a newline or NUL.

- [ ] **Step 3: Implement.**
  - **`--server CONFIG`** copies CONFIG into the run's I/O directory and starts `fenji-server --config <copy> run` in place of the stub. The server's stdout and stderr go to `re/runs/out/NAME/server.log`, run with `RUST_LOG=fenji_server=debug`. Readiness waits for both of its ports, read from the config's `login_bind` and `api_bind`; a failure exits 7, as for the stub.
  - **`--server-data DIR`** must resolve under `re/runs/`, and is bound read-write. The config's `[store] path` points into it, at a path the sandbox sees.
  - **`--launch "ARGS"`** runs `fenji-launch play --print ARGS` inside the sandbox once the server is ready. Only `gj_*` pairs from its stdout go into the client's environment. `inputs.txt` records the launch arguments and the pairs with `gj_ticket` redacted. A failure exits 7, and the client is not started.
  - **`xkey.py --display :99 (--key NAME | --type TEXT)`**, stdlib `ctypes` with libX11 and libXtst like `xclick.py`. `--key` sends one keysym (`Return`, `space`, `Tab`, `BackSpace`, a letter). `--type` sends ASCII text one key at a time, with Shift for upper case. Anything outside printable ASCII is refused.
  - The header documents the new options and what a run records.

- [ ] **Step 4: Run `scripts/spike/test-run-client.sh` and `shellcheck scripts/spike/*.sh`; expect PASS. Update `docs/re/README.md`'s "Running the client" with the new options. Commit; PR; review; merge.**

```bash
git add scripts/spike/run-client.sh scripts/spike/xkey.py scripts/spike/test-run-client.sh docs/re/README.md
git commit -m "feat(tools): let client runs use fenji-server, the launcher's ticket and key presses" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: The rungs (W9)

**Files:**
- Local: `re/m3/runs.md` (predictions and outcomes), `re/m3/run/fenji.toml`, `re/runs/server-data/` (the run database)
- Create: `crates/fenji-server/tests/fixtures/l3-create.hex` (the real client's 304 body), `crates/fenji-server/tests/real_client.rs`
- Modify: `docs/protocol/packets/lobby.md` (a Runs section), `docs/protocol/login.md` (§7: the launcher path with a Fenji ticket), `docs/protocol/opcodes.md` (statuses to `verified`)

Each run follows the Global Constraints' run rules. Before each run, write its prediction in `re/m3/runs.md`; after it, the outcome, the evidence paths and the watchdog result.

- [ ] **Step 1: Prepare.**

```bash
[ -S /tmp/.X11-unix/X99 ] || (Xwayland :99 -geometry 1280x720 -ac -nolisten tcp -noreset -extension MIT-SHM -shm >/dev/null 2>&1 &)
cargo build --release -p fenji-server -p fenji-stub
cargo build --release -p fenji-launcher --features dev-login-path --bin fenji-launch
mkdir -p re/m3/run re/runs/server-data
```
In `re/m3/run/fenji.toml`, set:
- `[server] name = "Fenji"`, `public_host = "127.0.0.1"`, `login_bind = "127.0.0.1:7800"` and `api_bind = "127.0.0.1:7801"`;
- `[store] path`: the sandbox's path to `re/runs/server-data/fenji.db`.

Create the account outside the sandbox: the server is ours, so running it on the host is fine, and the client never runs there. Start `target/release/fenji-server --config re/m3/run/fenji.toml run` in the background, then:
- `fenji-server … invite create --label m3-runs`;
- `FENJI_PASSWORD=… target/release/fenji-launch redeem --server http://127.0.0.1:7801 --invite <code> --username tester`;
- stop the server.

The password lives in `re/m3/run/password` (mode 600) and is bound read-only for `--launch`.

- [ ] **Step 2: L1, the handoff.** Prediction: the client connects about 10 s in; the server logs "ticket accepted" for `tester` and the move to `WORK`; the client sends 402; the title shows "Press Any Key". Run:

```bash
scripts/spike/run-client.sh run m3-l1 --timeout 180 --server re/m3/run/fenji.toml --server-data re/runs/server-data \
  --launch "--server http://127.0.0.1:7801 --username tester --password-file <sandbox path of re/m3/run/password>" \
  -- --gf-sdk=mocked --fs=data:_index:_index
```
Check:
- the server log;
- that `inputs.txt` shows `gj_ticket=<redacted>`;
- the screenshots;
- that a second connection with the same ticket would be refused (covered by the bot; nothing to run);
- the watchdog.

- [ ] **Step 3: L2, the list.** Same as L1, plus a key press once the title is up (from L1's screenshots, about 120 s in: `--key 130:Return`) and a timeout of 240 s. Prediction: the server logs 370, 496, 292 and answers 228 with no actors; about 4 s later the client shows character creation. If the title ignores the key (the intro-video gate), add a click on the window before the key.

- [ ] **Step 4: L3, creating a character.** Repeat L2. Read the creation screen's layout from L2's screenshots, then script the minimum with `--click`, `--type` and `--key`, timed from the screenshots:
  - pick a vocation;
  - continue through the screens;
  - click the name box and `--type …:Lin`;
  - press the final create button.

  Prediction: the server logs 304 (decoded: name `Lin`, vocation, skeleton, the face string's length), stores it, answers 628, and the select screen shows "Lin". Capture the 304 body from `server.log` (trace) into `crates/fenji-server/tests/fixtures/l3-create.hex`. It holds no machine data: the name is ours, and the face string comes from the client's face editor. Iterate runs (`m3-l3b`, …) until it passes, each with its prediction.

- [ ] **Step 5: L4, persistence.** A new run with the same `--server-data`, so a new server process starts on the same database. Press a key at the title. Prediction: 228 lists `Lin`, and the select screen shows it after the transition.

- [ ] **Step 6: L5, deletion.** In L4's select screen:
  - click the delete button (from L4's screenshots);
  - click the dialog's box;
  - `--type …:Swords of Legends Online`;
  - confirm.

  Prediction: the server logs 166 with Lin's pid, deletes it, answers 84 (pid, 0, 2147483647); the client sends 370, 496 and 292; 228 is empty; creation opens.

- [ ] **Step 7: L6, no SDK flag.** L1 without `--gf-sdk=mocked`. Prediction: unknown, and recorded either way: it starts and reaches "Press Any Key", or it exits early in SDK initialisation. Comment the result on #54 and #114.

- [ ] **Step 8: L7 (optional; only if time allows before the release).** `fenji-server` gains the cargo feature `dev-mocked-sdk`:
  - `[dev] mocked_account = "tester"` maps the mocked SDK's ticket to that account;
  - the server refuses to start with it unless `login_bind` is loopback;
  - a test, `default_build_rejects_mocked_tickets`, already exists in Task 6.

  Then a server-list run as M2's m2-r5 did, with `serverlist_url` and the two clicks, plus a key press and the L2 checks. If skipped, record that in the ledger and in m3-summary.

- [ ] **Step 9: The fixture test** (`crates/fenji-server/tests/real_client.rs`): decode `fixtures/l3-create.hex` with `Up::decode(0xFF, …)`, parse it with `CreateRequest::parse`, and check the name, vocation and skeleton L3 recorded. Then run it through the bot's server (create, then list), and check the 628 decodes.

- [ ] **Step 10: Write up.**
  - `lobby.md` gets §14 Runs: a table per run, with Observed, Inferred and Unknown after it, and evidence keys `M3L1`… into `re/runs/out/…` and `re/m3/runs.md`.
  - The ids seen on the wire become `verified` in `opcodes.md`.
  - `login.md` gets §7, "M3: the launcher path with a Fenji ticket": the handoff, its threat model (D3), and L1 and L6.
  - Record in `lobby.md` which names the client looked up and which URLs it tried, from `wine.log`'s winsock lines, for PRIVACY.md (Task 12).

  Commit (docs plus the fixture and its test); PR; review; merge.

```bash
git add docs/protocol/packets/lobby.md docs/protocol/login.md docs/protocol/opcodes.md crates/fenji-server/tests/fixtures/l3-create.hex crates/fenji-server/tests/real_client.rs
git commit -m "docs(re): run the lobby against the real client: list, create, restart, delete" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: The stub's ticket hygiene and `--work-pad` (W11, #102)

**Files:**
- Modify: `tools/stub/src/main.rs`, `tools/stub/src/speak.rs`, `tools/stub/src/session.rs`, `tools/stub/tests/speak.rs`

- [ ] **Step 1: Write the failing tests** (`tests/speak.rs`, with the existing fake client):
  - With `--speak --no-auth-capture`:
    - no `*.auth.bin` is written;
    - the session log holds the hello and a line `[auth frames not captured: N bytes]`, and none of the client's auth bytes (assert the payload's ticket string isn't in the log, hex or raw);
    - the `.events` line reads `auth (not captured) payload_len=N`.
  - `--no-auth-capture` without `--speak` exits 2.
  - With `--work-pad=300`, the stub's `Server` ends in `Work`: check the events log's state line, or expose `State` in the events, whichever the code allows.
- [ ] **Step 2: Implement.**
  - `SessionLog` gains a "hold" mode. Inbound bytes are buffered, not written, until `speak` reports that the auth message is complete. Then the log gets the marker line and resumes with the bytes after the auth frames: the frame reader reports how many bytes the auth took, through `Server::receive` consuming exactly that.
  - `--work-pad` sends its padded message through a `Server::begin_work_with(body, codec)`, which sets `Work` as `begin_work` does. Add that to `fenji-proto` with a unit test.
- [ ] **Step 3: Full gate; commit; PR (`Closes #102`); review; merge.**

```bash
git add tools/stub crates/fenji-proto/src/handshake.rs crates/fenji-proto/tests/handshake.rs
git commit -m "fix(tools): let the stub keep no auth capture, and end --work-pad in WORK" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: `apps/launcher`, the Tauri shell (W10)

**Files:**
- Create:
  - `apps/launcher/package.json`, `pnpm-lock.yaml`, `index.html`, `vite.config.ts`, `tsconfig.json`, `src/main.ts`, `src/api.ts`, `src/styles.css`;
  - `apps/launcher/src-tauri/Cargo.toml`, `Cargo.lock`, `build.rs`, `tauri.conf.json`, `capabilities/default.json`, `src/main.rs`, `src/commands.rs`, `icons/*`;
  - `apps/launcher/deny.toml`, `apps/launcher/README.md`.
- Modify: `.github/workflows/ci.yml` (a `launcher` job), root `Cargo.toml` (`exclude = ["apps/launcher/src-tauri"]`), `.gitignore` (`apps/launcher/node_modules/`, `apps/launcher/dist/`)

- [ ] **Step 1: Design with the `impeccable` skill.** Invoke it, give it the brief, and keep its decisions in `apps/launcher/README.md` ("Design"). The brief:
  - **Four views:**
    1. **Server:** the address, or paste an invite link.
    2. **Account:** sign in, or redeem an invite with a username and password.
    3. **Client:** the game folder, plus Quick and Full verification with per-file results.
    4. **Play:** disabled, showing `GATE_NOTICE`.
  - **One window, no telemetry**, desktop only (Linux and Windows).
  - **Tone:** a small private server for friends, not a storefront.
  - **No game art or logos:** our own simple mark only (ground rule 3; trademarks).
- [ ] **Step 2: Scaffold.**
  - `src-tauri` is its own workspace (`[workspace]` in its `Cargo.toml`), depending on `fenji-launcher = { path = "../../../crates/fenji-launcher" }` without `dev-login-path`, `tauri = "2.12"` and `tauri-build = "2.5"`.
  - Its `deny.toml` copies the root's and adds `[[licenses.exceptions]]` for exactly `option-ext`, `cssparser`, `cssparser-macros`, `selectors` and `dtoa-short` with `allow = ["MPL-2.0"]` (D20).
  - `tauri.conf.json`: no plugins; `app.security.csp = "default-src 'self'; style-src 'self'; script-src 'self'; connect-src ipc: http://ipc.localhost"`. `capabilities/default.json` grants only `core:default` and the app's own commands.
  - **Icons:** generate a 1024×1024 PNG of our own simple mark with a stdlib Python script, kept as `apps/launcher/icon-source.py` and run with `python3 -I`, then `cargo tauri icon`.
- [ ] **Step 3: Commands with tests first** (`src/commands.rs`, `#[tauri::command]` wrappers over `fenji-launcher`; the logic lives in the library, so these are thin):
  - `server_info(url)`, `redeem(url, invite, username, password)`, `sign_in(url, username, password) -> SessionView` (the session token stays in Rust state, never returned to the webview), `verify(client, full) -> ReportView`, `settings_load()`, `settings_save(…)`, `play_status() -> { available: false, notice }`;
  - unit tests for the view conversions and for `play_status` in the default build.
- [ ] **Step 4: Frontend.** TypeScript in `src/main.ts`, calling `invoke` through `src/api.ts`. Build it with `pnpm install && pnpm build`, then `cargo build` in `src-tauri`; a GUI start isn't needed for the tests.
- [ ] **Step 5: CI.** A `launcher` job on `ubuntu-24.04`:
  - `sudo apt-get install -y libwebkit2gtk-4.1-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev`;
  - pnpm (the pinned version), `pnpm install --frozen-lockfile`, `pnpm build`;
  - in `apps/launcher/src-tauri`: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, `cargo deny check`, using EmbarkStudios' action with `manifest-path`.
- [ ] **Step 6: Commit; PR; review; merge.** If this task would put the release at risk (time), the plan allows splitting it into a follow-up issue in M4 (spec W10), and the morning report says so.

```bash
git add apps/launcher .github/workflows/ci.yml Cargo.toml .gitignore
git commit -m "feat(launcher): add the launcher's desktop shell" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Records (W12)

**Files:**
- Create: `docs/re/m3-summary.md`
- Modify: `docs/superpowers/specs/2026-10-11-fenji-design.md` ("M3 update" notes in §5, §10, §12, and §12's "Answer (M3)" column), `docs/README.md`, `README.md` (status table, what's in the repository), `PRIVACY.md` (the game connection's threat model, the lobby's lookups and URLs, what the server stores now), `SECURITY.md` (v0.4.0's server is for testing; nothing meant to face the internet until M4's TLS), `CLAUDE.md` (Status line), `docs/re/README.md` if Task 8 didn't cover it

- [ ] **Step 1: `m3-summary.md`** in `m2-summary.md`'s form: what M3 delivered, open questions answered (with "Where"), deviations from the spec and the plan (with how each ended), Done-when with status and evidence, gates and rulings (G1 #114 and its options; L6's answer), and findings M4 inherits (the world's ids 772 and 543, the time sync #91, `prop_sys_equips` from the client's tables, slots (321) and entering the world).
- [ ] **Step 2: Parent spec notes** ("M3 update" in §5.1–§5.3, §10 and §12; the M3 answers for the ticket question and the one-or-two-connections question).
- [ ] **Step 3: README, PRIVACY, SECURITY, docs index, CLAUDE.md status.** In plain words:
  - the game connection is readable and unauthenticated; no password crosses it; use a VPN between friends if that matters;
  - what the lobby screens look up (from Task 9);
  - the server's account data, as the code now stores it.
- [ ] **Step 4: Commit; PR; review; merge.**

```bash
git add docs/re/m3-summary.md docs/superpowers/specs/2026-10-11-fenji-design.md docs/README.md README.md PRIVACY.md SECURITY.md CLAUDE.md
git commit -m "docs: record M3: summary, spec notes, privacy and security" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 13: Final review, fix pass and release v0.4.0

- [ ] **Step 1: The final review.** Dispatch one fresh reviewer (opus) on the whole milestone: `git diff <dev at M3 start>..dev`, the spec, this plan and the ledger. It checks the ground rules, D1–D20, the threat model, secrets in logs, and each Done-when item against its evidence.
- [ ] **Step 2: One fix pass.** Fix Critical and Important findings, one PR per concern, each reviewed before it merges. Record minors as deferred, in the ledger and m3-summary.
- [ ] **Step 3: Release preparation** (`docs/development.md`, "Releasing"):
  - cut `release/v0.4.0` from `dev`;
  - bump `[workspace.package] version` to 0.4.0, plus `apps/launcher`'s `tauri.conf.json`, `package.json` and `src-tauri/Cargo.toml`;
  - `cargo check --workspace`;
  - set CLAUDE.md's **Status**;
  - `git cliff --tag v0.4.0 -o CHANGELOG.md`;
  - stage only the release files, and commit `chore(release): v0.4.0`;
  - open the PR into `main`.
- [ ] **Step 4: The release, only under Ruling R0's conditions:** M3's minimum done met, the final review clean, CI green. Then:
  - merge with a merge commit;
  - tag `v0.4.0`;
  - merge `main` back into `dev`;
  - `gh release create v0.4.0` with the pinned-build line and the changelog section, plus a line that this release isn't for players (D2, #114);
  - close the release issue and milestone M3, after moving open items such as #91 to M4.

  Otherwise leave the PR open and say why in the morning report.
- [ ] **Step 5: The morning report**, in the ledger and as the final message:
  - what's done (PRs, issues) and what's in progress;
  - every ruling with its cost;
  - the gates opened;
  - the deferred minors;
  - test and CI status;
  - exactly what the owner needs to do next.

---

## Amendment A: review fixes and execution rulings (2026-10-13)

The plan's review (PR #128) and the first PR reviews changed these points. Each change is a ledgered ruling; where this amendment and a task's text differ, this amendment wins.

**From the lobby reading, before execution.** Task 4 already uses the reading's values (ids from the client's protocol table, the 228/628/84/270 shapes, the actor fields, the Western create rules), so Task 1 writes the evidence for them rather than new values. The reading's script-side checks were confirmed by the controller: the client's protocol table, decoded directly, names every id Task 4 uses. Two values differ from the plan's first text, and both are fixed in Task 4's PR (#131):
- the base face and the outfit are **clamped** to 1–2, as the client's `check_base_face` (face_verification.luac `0.36`) does: 7 gives 2;
- names allow only the diacritics the Western rule names: umlauts, grave and acute accents, and cedillas.

**Task 2, the limiter (final design, after six background security reviews; PR #129).**
- `Limiter::begin(username, addr, now) -> Verdict` and `Limiter::succeed(Attempt)`. `Verdict::Allow(Attempt)` carries the attempt; `Locked` and `SlowDown` carry `retry_after`.
- Every attempt counts when it begins, and a success takes back exactly its own attempt.
- Pairs (username hash, network) lock after 5 in 15 min, and networks (IPv6 by /64) after 20.
- A username's slow-down counts in one of 4096 buckets of a SipHash keyed per process, so the limiter never learns whether an account exists.
- Live entries are never dropped, sweeps run at most once a second, and past 4×`max_keys` a new pair or network isn't counted on its own (ruled: no global lockout).

**Task 5 (`Accounts::sign_in`).**
- Call `limiter.begin(username, addr, now)` first, before the account lookup and the password check, and `succeed(attempt)` only on success. Map both `Locked` and `SlowDown` to the one "try again later" answer.
- `SessionGrant` and `GameGrant` get hand-written redacting `Debug` impls, each with a `format!("{x:?}")` test (review I1).
- `register` checks that the invite is open (a read-only `Store::invite_is_open(code_hash, now)`) before hashing the password, and keeps the transactional check. An invalid invite counts against the address through the limiter (`begin` under the invite's own key), so fake codes can't keep argon2 busy. Test: a bad invite is answered without hashing (review I4).
- `TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, …)`; `TimeoutLayer::new` is deprecated in tower-http 0.7.
- The client address comes from `X-Forwarded-For`'s last entry only (if it doesn't parse, the peer's address), and only from a trusted peer.
- `retry_after_secs` is rounded up, never 0.
- Start-up warns when `api_bind` is loopback and `trusted_proxies` is empty, since behind a local Caddy every request comes from 127.0.0.1.
- `cli_writes_while_a_server_holds_the_database` holds a `BEGIN IMMEDIATE` on a second connection for about 200 ms while the CLI writes.

**Task 6.**
- The gate sends every script message through one function that refuses `lobby::NEVER_SEND` ids, with a test.
- Logging (review I2):
  - a `debug` line per 304: name, vocation, skeleton, face length, and the `Refusal` variant when refused;
  - a `trace`-level hex dump of each inbound gateway script body after auth (never the auth message);
  - the ticket field is never logged.
- `RunningServer` fills `ServerInfo.gate_port` from the bound address, and the accept loop reaps finished connections (`join_next`).
- Tests: the 64-connection pre-auth cap, a direct `lobby::handle` call without sockets (spec §4), and byte-exact 228, 628, 84 and 270 encodings (spec §6).
- `ClientEvent::Rejected` and `Closed` carry bytes: the bot compares with `AUTH_FAILED.as_bytes()`.

**Task 7.**
- `ApiClient` builds reqwest with `redirect(Policy::none())`, so a redirect can't carry a password to `http://`.
- `Session` and `GameGrant` get redacting `Debug` impls.
- `play --print` needs no base spec. It prints only the login path's `gj_*` pairs, and signs out after taking the game token (S16).
- Step 4 also runs `fenji-launch verify --client "$FENJI_CLIENT_PATH"` with and without `--full`, and records the result (Done-when 5).
- CI gets `cargo clippy` and `cargo test` with `-p fenji-launcher --features dev-login-path`. The default-build test is `#[cfg(not(feature = "dev-login-path"))]`.

**Task 8.**
- `--launch-password FILE` is bound read-only at its own path. It is never copied into the run's I/O directory and never written to `inputs.txt`, with a test (review I3). The run account's password is random per setup.
- The server writes `$IO/server.log`, which the runner collects. Runs use `RUST_LOG=fenji_server=trace`.

**Task 9.**
- L3's fixture keeps the real 304's shape with a synthetic face string of the same form, since the real one may be the client's built-in defaults.
- L7's default-build test is `expired_mocked_and_garbage_tickets_are_refused` (Task 6).
- **Stop rule:** if L3 or L5 still fails after six runs, write up what the runs showed, record it in m3-summary as open, and go on to Tasks 10–13. The release decision (Ruling R0) then reflects it.

**Wording.** The stub captures the auth unless `--no-auth-capture` is given. D3's "never logs the ticket" is about Fenji's server, runner and launcher, plus the stub with that flag.

**Spec deviations to record in m3-summary (Task 12).**
- D8's value tree and cycle guard became typed scalars, because the create request carries only scalars (ruling P1).
- D16's `[data] client_path` is parsed but not read in M3 (ruling P4: `prop_sys_equips` is empty).
- `PRE_AUTH_CAP` is 26 + 4 + 0x4000 + 512 rather than D17's 26 + 0x4000, to allow for the header, frame lengths and time syncs.
- The `invites` table's key is `id`, with `code_hash` unique.
- The limiter's final design (above) replaces §4's "per username" with keyed buckets.
