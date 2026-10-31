# Retranslation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** translate every line of the game's English anew from its Chinese, with the tools that make, check and assemble it, `fenji-content`, which holds it, and the launcher's part in fetching it.

**Architecture:**
- **`crates/fenji-pack`:** the content pack's format: fingerprints, entries, the manifest, reading a pack and checking its signature.
- **`tools/translate`:** a dev tool that reads the pinned copy and writes only under `re/retranslation/`. It:
  - extracts the lines;
  - holds the glossary;
  - plans the batches;
  - checks answers;
  - drives headless Claude Code runs on the owner's subscription;
  - assembles the new English into a `fenji-content` checkout.
- **The launcher** fetches and checks the pack. The patcher's own plan (#336) applies it, so the launcher's changes land with 0.6.0.

**Tech Stack:**
- Rust 2024: the workspace's crates, clap, serde and serde_json, sha2, flate2, tar;
- `minisign-verify` 0.3, plus `minisign` for signing in tests;
- the Claude Code CLI (`claude -p`);
- GitHub Actions for `fenji-content`.

**Spec:** [`docs/superpowers/specs/2026-10-18-retranslation-design.md`](../specs/2026-10-18-retranslation-design.md). Section numbers below (§…) are the spec's, and RT1–RT16 and D1–D8 are its rulings and decisions.

## Global Constraints

- **Ground rules 1–4, 6 and 7 hold, rule 3 as amended:**
  - no Chinese line, and no line of the game's original English, in any repository;
  - `fenji-content` holds only the new English, keyed by fingerprints.
- **A fingerprint** is the first 16 bytes of the SHA-256 of a line's exact source bytes, as the locale map holds them. It is written as 32 lowercase hex characters (§6.2), and nothing normalizes the bytes before hashing.
- **What comes from the copy stays in `re/retranslation/`,** which is git-ignored. It never goes to cloud agents or into a repository (§3).
- **The pinned build is 10015727.** The tools refuse another exe unless given `--any-build`, as `luatool` does.
- **At most two agents at once.** The driver's headless runs count. While it runs two, sessions dispatch no subagents.
- **American spelling** (D3).
- **Tests use invented lines only,** in Chinese or English, never a line from the game.
- **The gate before each PR:** `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny check`, plus `scripts/check-forbidden-files.sh` and `scripts/check-unsafe-allowed.sh`. For the launcher, also its own checks (`docs/development.md`, "The desktop launcher").
- **Commits:**
  - Conventional Commits with one `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` trailer, never a session link;
  - made with `TZ=UTC`, as `git commit`, `git merge` and `git tag` are now;
  - one PR per task, squash-merged into `dev`.
- **0.5.x keeps the unmodified client (D8).** Tasks 1–13 change only tools, docs and `fenji-content`, so they may merge into `dev` while M4's 0.5.x releases go on. Task 14 changes the shipped launcher, so it merges with the patcher's hook, for 0.6.0.

## Review Focus

1. **A source whose own markup is unbalanced,** such as a `[c …]` with no `[/c]`. The checker compares the translation's tokens with the source's, and never demands markup the source lacks (Task 4).
2. **Sources that differ only in whitespace, escapes or a trailing space.** They are different lines with different fingerprints. Nothing trims or normalizes before hashing, and the client's own map is the arbiter (Tasks 1 and 3).
3. **A headless answer that is truncated, isn't JSON, misses a line, adds a line or names a wrong fingerprint.** The whole answer is refused and the job retried. Nothing partial is ever kept (Task 7).
4. **The subscription's usage limit mid-run, a crash or a reboot.** No job is marked failed for it, nothing half-written stays, and the driver resumes where it was (Task 7).
5. **One Chinese line used in several places,** for instance a label that's also a word in prose. It keeps one translation, and the strictest length budget of its uses applies (Tasks 3 and 4).

---

## Phase A: the tools (repository-only, inline)

### Task 1: `fenji-pack`, the content pack's format

**Files:**
- Create: `crates/fenji-pack/Cargo.toml`, `crates/fenji-pack/src/lib.rs`, `crates/fenji-pack/src/fingerprint.rs`, `crates/fenji-pack/src/tsv.rs`, `crates/fenji-pack/src/pack.rs`
- Modify: `Cargo.toml` (members; `minisign-verify = "0.3.0"` in workspace dependencies), `deny.toml` only if a new licence appears

**Interfaces:**
- Produces:
  - `pub struct Fingerprint(pub [u8; 16])`, with `Display` (32 lowercase hex) and `FromStr` (exactly 32 hex digits, either case);
  - `pub fn fingerprint(source: &[u8]) -> Fingerprint`;
  - `pub struct Entry { pub fingerprint: Fingerprint, pub english: String }`;
  - `pub fn escape(text: &str) -> String` and `pub fn unescape(field: &str) -> Result<String, PackError>`;
  - `pub fn write_tsv(entries: &mut [Entry]) -> String`, which sorts the entries by fingerprint;
  - `pub fn read_tsv(text: &str) -> Result<Vec<Entry>, PackError>`;
  - `pub const FORMAT: u32 = 1`;
  - `pub struct Manifest { pub format: u32, pub build: String, pub version: String, pub lines: u64, pub files: BTreeMap<String, String> }`, serde JSON, mapping each file's path to its SHA-256 hex;
  - `pub struct Pack { pub manifest: Manifest, pub english: HashMap<Fingerprint, String> }`;
  - `pub fn read_pack(bytes: &[u8]) -> Result<Pack, PackError>`;
  - `pub fn verify(pack: &[u8], signature: &str, public_key: &str) -> Result<(), PackError>`;
  - `pub enum PackError`, a thiserror enum.

- [ ] **Step 1: Write the failing tests** in `src/fingerprint.rs`, `src/tsv.rs` and `src/pack.rs`.
  - `fingerprints_are_the_first_16_bytes_of_sha256`: `fingerprint(b"abc").to_string() == "ba7816bf8f01cfea414140de5dae2223"`, and `"BA7816BF8F01CFEA414140DE5DAE2223".parse::<Fingerprint>()` gives the same value.
  - `nothing_normalizes_a_source`: `fingerprint("你好".as_bytes()) != fingerprint("你好 ".as_bytes())`.
  - `escapes_round_trip_and_unknown_escapes_are_refused`:
    - `escape("a\tb\nc\\d\re") == r"a\tb\nc\\d\re"`, and `unescape` gives the original back;
    - `unescape(r"a\qb")` is `Err`.
  - `tsv_is_sorted_and_refuses_duplicates_and_disorder`:
    - `write_tsv` puts lines in fingerprint order, one `<fp>\t<escaped>\n` each;
    - `read_tsv` refuses a repeated fingerprint, lines out of order, a missing tab and bad hex.
  - `a_pack_is_read_only_when_every_file_matches_its_manifest`. Build a `.tar.gz` in the test from `manifest.json` and `text/en/items.tsv`, holding invented entries. Then:
    - `read_pack` gives the manifest and the map;
    - a changed byte, an extra entry, a `../` path, a fingerprint in two files, or `lines` not equal to the count each give an `Err`.
  - `a_pack_is_verified_by_its_minisign_signature`. Sign it in the test with the `minisign` dev-dependency, using a key made there. Then:
    - `verify` passes;
    - a changed byte fails;
    - another key's signature fails.
- [ ] **Step 2: Run them to verify they fail.** `cargo test -p fenji-pack`. Expected: compile errors, since nothing is defined yet.
- [ ] **Step 3: Implement.**
  - `fingerprint` is `sha2::Sha256` truncated to 16 bytes.
  - TSV escapes are exactly `\\`, `\t`, `\n` and `\r`.
  - `read_pack` reads the tarball with `flate2` and `tar`:
    - at most 256 MiB compressed and 1 GiB unpacked, with only `manifest.json`, `text/<lang>/*.tsv` and `glossary.tsv` as entries;
    - every file's SHA-256 checked against the manifest;
    - `FORMAT` checked.
  - `verify` wraps `minisign_verify::{PublicKey, Signature}`.
- [ ] **Step 4: Run them to verify they pass.** `cargo test -p fenji-pack`. Expected: all pass.
- [ ] **Step 5: Run the gate, then commit.** `feat(content): fenji-pack, the content pack's format and signature`.

### Task 2: reading a copy's file by its path, in `fenji-data`

**Files:**
- Create: `crates/fenji-data/src/client.rs`
- Modify: `crates/fenji-data/src/lib.rs` (`pub mod client;`), `tools/archive/src/main.rs` (it uses `client`, with its own `decoded_index`, `live_places` and `read_record` moved there)
- Test: `crates/fenji-data/src/client.rs` (unit tests); `tools/archive/tests/cli.rs`, unchanged, must still pass

**Interfaces:**
- Produces:
  - `pub struct Copy { /* the copy's root, its index, its live places */ }`;
  - `impl Copy { pub fn open(root: &Path) -> Result<Copy>; pub fn read(&self, path: &str) -> Result<Vec<u8>>; pub fn paths(&self) -> impl Iterator<Item = &str> }`.
  - `read` decodes stored, LZ4 and Oodle records with `BuiltinCodecs`, and gives `Error::NotFound(path)` for a path `_index` doesn't name or no live record holds.

- [ ] **Step 1: Write the failing test** `a_copy_reads_a_file_by_path_through_its_index`. Build a copy with `test_util` (`zfs_part`, `stored_blob`, `oodle_blob`) whose `_index` names two invented paths, one stored and one Oodle. Then:
  - `read` gives both contents;
  - an unknown path gives `NotFound`.
- [ ] **Step 2: Run it to verify it fails.** `cargo test -p fenji-data client`. Expected: `client` is not defined.
- [ ] **Step 3: Move the logic out of `tools/archive`.**
  - `decoded_index`, `live_places` (the first part's record wins) and `read_record` become `Copy`'s private parts.
  - The spool stays in the archive tool. `Copy` decodes natively only.
- [ ] **Step 4: Run the tests to verify they pass.** `cargo test -p fenji-data -p fenji-archive`. Expected: all pass, including the archive tool's CLI tests, unchanged.
- [ ] **Step 5: Run the gate, then commit.** `refactor(data): a copy's file by its path, in fenji-data, for the archive tool and the translation tools`.

### Task 3: `translate extract`, the lines and where they're used

**Files:**
- Create:
  - `tools/translate/Cargo.toml`: package `fenji-translate`, binary `translate`;
  - `tools/translate/src/main.rs`;
  - `tools/translate/src/lib.rs`;
  - `tools/translate/src/lines.rs`;
  - `tools/translate/src/extract.rs`;
  - `tools/translate/src/area.rs`;
  - `tools/translate/tests/extract.rs`.
- Modify: `Cargo.toml` (members)

**Interfaces:**
- Consumes:
  - `fenji_data::client::Copy` (Task 2), `fenji_data::cache`, `fenji_data::re_dir::resolve_out_dir`;
  - `fenji_luatool::client::{exe_path, load, decode}`, `fenji_lua::chunk::{parse, Constant}`;
  - `fenji_pack::fingerprint` (Task 1).
- Produces:
  - `pub struct Line { pub fp: Fingerprint, pub zh: String, pub en: Option<String>, pub uses: Vec<Use>, pub area: Area, pub tight: bool }`, serde, one per line of `re/retranslation/lines.jsonl`;
  - `pub enum Use { Table { sheet: String, row: String, field: String }, Script { path: String }, Tab { path: String, column: String } }`;
  - `pub enum Area { Interface, Items, Skills, Quests, Dialogue, Lore, Other }`, with `fn name(self) -> &'static str`, which is the file name in `text/en/`;
  - `pub fn extract(copy: &Copy, exe: &PeImage) -> Result<Vec<Line>>`;
  - `pub fn read_lines(work: &Path) -> Result<Vec<Line>>`.
  - The CLI: `translate extract [--client DIR] [--any-build] [--out re/retranslation]`. It writes `lines.jsonl` and `report.txt`, the counts of §5's kind.

- [ ] **Step 1: Write the failing test** `extract_joins_the_map_the_tables_and_the_scripts`, in `tests/extract.rs`. Build a copy and an exe in the test from invented data:
  - a `locale_en` cache whose main table maps three invented Chinese lines to English;
  - an `all_tables` cache whose `items` sheet holds two `LString`s, `L("…")`, one of them a line that's not in the map;
  - an exe whose embedded Lua chunk has a third line as a string constant.

  Then assert:
  - four `Line`s;
  - each `fp` is `fingerprint` of its `zh` bytes;
  - the line missing from the map has `en: None`;
  - the table lines carry `Use::Table { sheet: "items", row, field: "name" | "desc" }`;
  - the script line carries `Use::Script`.
- [ ] **Step 2: Write the failing tests** `escapes_in_an_lstring_unescape_as_lua_does`, `sources_differing_by_a_trailing_space_stay_two_lines` and `areas_and_tightness_follow_the_uses`.
  - The first: `\n`, `\"`, `\\`, `\ddd` and `\x41` in an `L("…")` argument read as Lua 5.3 reads them.
  - The second covers four cases:
    - a script use is `Interface`, `tight`;
    - `items`' `name` is `Items`, `tight`;
    - `items`' `desc` is `Items`, not tight;
    - a line used both in a script and in a quest's prose takes the first area in `Area`'s order, and is tight.
- [ ] **Step 3: Run them to verify they fail.** `cargo test -p fenji-translate`. Expected: compile errors.
- [ ] **Step 4: Implement.**
  - **The map:** the locale cache's largest string-to-string table, as §5's count found it.
  - **The tables:** every `LString` in `all_tables`, by sheet, row key and field. The non-`L` forms (`A(…)`, `E(…)`) record their inner `L("…")`.
  - **The scripts:** each Lua chunk's string constants, through every proto. A constant that's a key of the map adds a `Script` use.
  - **`.tab` files:** every `.tab` under `data/tables` that `_index` names. Cells holding CJK add `Tab` uses.
  - **Areas:** `area.rs` holds `AREAS: &[(&str, Area)]`, sheet-name prefixes that Step 4 sets from the 339 sheet names. The other sheets are `Other`. `TIGHT_FIELDS: &[&str]` covers `name`, `title`, `label`, `button`, `tab` and `tip`.
  - **The CLI** checks the exe against `PINNED_EXE_SHA256`, as `luatool` does.
- [ ] **Step 5: Run them to verify they pass.** `cargo test -p fenji-translate`. Expected: all pass.
- [ ] **Step 6: Run it on the pinned copy, locally.**
  - `cargo run -p fenji-translate --release -- extract`. Expected:
    - `re/retranslation/lines.jsonl` holds about 175,415 + 9,100 lines. It holds 176,282: 175,415, plus 39 in the map's contexts, plus 828 table lines with no English (Task 3's ruling);
    - `report.txt`'s counts agree with §5 within the escapes' difference.
  - Ledger the counts. They are numbers only.
- [ ] **Step 7: Run the gate, then commit.** `feat(tools): translate extract, every line with where it's used`.

### Task 4: the markup, and the checks on a translation

**Files:**
- Create: `tools/translate/src/markup.rs`, `tools/translate/src/check.rs`, `tools/translate/src/british.txt` (our own list of British spellings, one per line, with their American forms)
- Modify: `tools/translate/src/lib.rs`

**Interfaces:**
- Consumes: `Line` (Task 3), `Glossary` (Task 5's type, defined here as a dependency-free struct that Task 5 fills)
- Produces:
  - `pub struct Token { pub family: Family, pub shape: String, pub visible: Vec<String> }`. `shape` is the raw token, with any visible text replaced by `\u{1}`;
  - `pub enum Family { Bracket, Brace, Html, Code, LineBreak }`;
  - `pub fn tokens(s: &str) -> Vec<Token>`;
  - `pub struct Glossary { pub terms: Vec<Term> }`;
  - `pub struct Term { pub zh: String, pub en: String, pub kind: TermKind, pub variants: Vec<String>, pub note: String }`;
  - `pub enum TermKind { Name, Word }`;
  - `pub struct Rules { pub allowed: BTreeSet<char>, pub budget_percent: u32 }`, with two constructors:
    - `Rules::from_lines(lines: &[Line]) -> Rules`: printable ASCII, a line break and a tab, plus every non-CJK character today's English uses, with `budget_percent: 110`;
    - `Rules::load(work: &Path, lines: &[Line]) -> Result<Rules>`: `from_lines`, overridden by `re/retranslation/rules.json` where Task 11 wrote one;
  - `pub enum Problem { MissingToken(String), ExtraToken(String), OutOfOrder(String), Cjk, Char(char), Term(String), TooLong { len: usize, budget: usize }, British(String), Empty }`;
  - `pub fn check(line: &Line, english: &str, glossary: &Glossary, rules: &Rules) -> Vec<Problem>`.

- [ ] **Step 1: Write the failing tests** for `tokens`, with invented strings for each family in §5:
  - `[c 99ccff]` and `[/c]`, `[size 12]`, `[indent]`, `[rolename]`;
  - `[q_npcname text="某人"]` and `[d_npc name="某人" action=""]`, whose visible text is `某人`;
  - `{0}`, `{0:d}`, `{1:.2f}`, `{target}`, `{color 99ff99 某词}` (visible `某词`);
  - `<font color="#dfdc9d">`, `</font>`, `<br>`, `<b>`;
  - `|cp,1,2|`, `#某词#`, a line break.

  Each test asserts the family, the shape, and the visible text.
- [ ] **Step 2: Write the failing tests for `check`:**
  - `a_good_translation_has_no_problems`;
  - `every_token_must_survive`: missing, extra, and a bracket or HTML open and close out of their order are refused. Brace placeholders and codes may move;
  - `visible_text_inside_markup_is_translated`: a translation that keeps `text="某人"` gives `Cjk`;
  - `an_unbalanced_source_is_matched_not_mended`: a source with `[c 99ccff]` and no `[/c]` passes with the same single token;
  - `glossary_names_are_required_as_written`: a line whose Chinese holds a `Name` term must contain its English, or a listed variant;
  - `the_strictest_budget_applies_to_a_tight_line`: `tight` with a 6-character old English and `budget_percent: 110`: 7 characters pass, 8 fail;
  - `british_spellings_and_unknown_characters_are_refused`: `colour` gives `British("colour")`; a curly quote not in `allowed` gives `Char('”')`; empty gives `Empty`.
- [ ] **Step 3: Run them to verify they fail.** `cargo test -p fenji-translate markup check`. Expected: compile errors.
- [ ] **Step 4: Implement** `tokens` as one scanner over the four families. The token grammar comes from the counts' shapes (§5). A shape the scanner can't place is its own `Code` token, so it survives as it is.
- [ ] **Step 5: Run them to verify they pass.** `cargo test -p fenji-translate`. Expected: all pass.
- [ ] **Step 6: Run the tokenizer over every source in `lines.jsonl`, locally.** Ledger the count of shapes per family, and of lines with tokens the scanner made `Code` of. Each of those is either taken into a family, or ledgered as a ruling.
- [ ] **Step 7: Run the gate, then commit.** `feat(tools): the markup's tokens, and the checks a translation passes`.

### Task 5: the glossary

**Files:**
- Create: `tools/translate/src/glossary.rs`
- Modify: `tools/translate/src/main.rs` (subcommands `glossary candidates`, `glossary export`)

**Interfaces:**
- Consumes: `Line` (Task 3), `Glossary` and `Term` (Task 4), `fenji_pack::{fingerprint, escape}` (Task 1)
- Produces:
  - `pub fn candidates(lines: &[Line]) -> Vec<Candidate>`, where `pub struct Candidate { pub zh: String, pub en_old: Option<String>, pub uses: usize, pub kind: TermKind }`, sorted by uses, most first;
  - `pub fn load(work: &Path) -> Result<Glossary>`, which reads `re/retranslation/glossary.json`;
  - `pub fn export(glossary: &Glossary) -> String`, which writes `glossary.tsv` for `fenji-content` as `<fp(zh)>\t<en>\t<kind>\t<note>`, with no Chinese.

- [ ] **Step 1: Write the failing tests:**
  - `candidates_are_names_in_markup_and_name_fields`: invented lines with `[q_npcname text="某人"]`, `[d_npc name="某人"]` and an `npcs` row's `name` give `某人` as a `Name` with its use count; a `maps` row's `name` gives a `Name` too;
  - `the_export_holds_no_chinese`: `export` of a glossary with a Chinese term holds the term's fingerprint and English only, and `assert!(!out.chars().any(is_cjk))`.
- [ ] **Step 2: Run them to verify they fail.** Then implement, run them to verify they pass, run the gate, and commit: `feat(tools): the glossary's candidates and its export`.

### Task 6: the batches, and the three jobs' prompts

**Files:**
- Create:
  - `tools/translate/src/batch.rs`, `tools/translate/src/prompt.rs`;
  - `tools/translate/prompts/style.md`: the style guide, §6.6, with invented examples. It's the one copy: Task 8 publishes it in `fenji-content` as `STYLE.md`;
  - `tools/translate/prompts/translate.md`, `review.md` and `settle.md`: instructions only, with no game text;
  - `tools/translate/prompts/*.schema.json`.
- Modify: `tools/translate/src/main.rs` (`batch`)

**Interfaces:**
- Consumes: `Line`, `Glossary`, `Rules` (Tasks 3–5)
- Produces:
  - `pub struct Batch { pub id: String, pub lines: Vec<BatchLine>, pub glossary: Vec<Term> }`;
  - `pub struct BatchLine { pub fp: Fingerprint, pub zh: String, pub en_old: Option<String>, pub context: String, pub tight: bool, pub budget: Option<usize> }`;
  - `pub fn plan_batches(lines: &[Line], glossary: &Glossary, rules: &Rules) -> Vec<Batch>`, which writes `re/retranslation/batches/<area>-<00001>.json`;
  - `pub const MAX_LINES: usize = 80` and `pub const MAX_CHARS: usize = 4_000` (Chinese characters per batch). A longer single line is a batch of its own;
  - `pub fn translate_prompt(b: &Batch, style: &str) -> String`, `pub fn review_prompt(b: &Batch, answer: &Answer, style: &str) -> String`, `pub fn settle_prompt(b: &Batch, disputes: &[Dispute], style: &str) -> String`;
  - `pub struct Answer { pub lines: Vec<AnswerLine> }`, where `pub struct AnswerLine { pub fp: Fingerprint, pub en: String }`;
  - `pub struct Review { pub lines: Vec<ReviewLine> }`, where `pub struct ReviewLine { pub fp: Fingerprint, pub keep: bool, pub en: Option<String>, pub reason: Option<String> }`;
  - `pub struct Dispute { pub fp: Fingerprint, pub first: String, pub second: String, pub reason: String }`;
  - `pub struct Settled { pub lines: Vec<SettledLine> }`, where `pub struct SettledLine { pub fp: Fingerprint, pub en: String, pub reason: String }`.

- [ ] **Step 1: Write the failing tests:**
  - `a_rows_lines_stay_together_and_batches_keep_their_limits`: an item row's `name` and `desc` share a batch; no batch holds more than `MAX_LINES` lines or `MAX_CHARS` Chinese characters but a single long line; script lines group by script path;
  - `a_batch_carries_only_the_terms_its_lines_hold`;
  - `the_prompts_hold_the_batch_the_style_and_no_other_line`;
  - `the_schemas_accept_a_good_answer_and_refuse_a_bad_one`: invented JSON against `translate.schema.json` and `review.schema.json`, checked with `serde_json` into `Answer` and `Review`.
- [ ] **Step 2: Run them to verify they fail.** Then implement, run them to verify they pass, run the gate, and commit: `feat(tools): batches by context, and the translate, review and settle prompts`.

### Task 7: the driver of headless runs

**Files:**
- Create: `tools/translate/src/drive.rs`, `tools/translate/tests/drive.rs`, `tools/translate/tests/fake-claude.sh`
- Modify: `tools/translate/src/main.rs` (`drive`, `status`)

**Interfaces:**
- Consumes: Tasks 3–6
- Produces:
  - `pub struct Driver { pub claude: PathBuf, pub model: String, pub effort: String, pub runs: usize, pub work: PathBuf, pub cwd: PathBuf, pub retries: u32 }`. `runs` is at most 2, the default model is `claude-opus-5-5`, the default effort `high`, and `retries` is 2;
  - `impl Driver { pub fn run(&self) -> Result<Summary> }`;
  - `pub struct Summary { pub translated: usize, pub reviewed: usize, pub settled: usize, pub queued: usize, pub waiting: bool }`;
  - `pub fn status(work: &Path) -> Result<Summary>`.
  - **Files under `work`:**
    - `answers/<id>.json`, `reviews/<id>.json` and `settled/<id>.json`, each written to a temporary file and renamed into place;
    - `queue/<id>.json`: lines that failed their checks after `retries`, for an interactive session;
    - `PAUSE`: if present, no new job starts.
  - **The command:** `claude -p --output-format json --json-schema <schema> --tools "" --strict-mcp-config --no-session-persistence --model <model> --effort <effort>`, run in `cwd`, a folder outside the repository. The prompt goes on stdin.

- [ ] **Step 1: Write the fake.** `fake-claude.sh` reads its mode from `FAKE_CLAUDE_MODE`, and appends its arguments and working directory to `FAKE_CLAUDE_LOG`. Its modes:
  - `ok`: echoes a valid answer for the fingerprints it finds in the prompt;
  - `short`: drops one line;
  - `wrong-fp`: answers one line under a fingerprint the batch doesn't hold;
  - `junk`: prints something that isn't JSON;
  - `limit`: the CLI's usage-limit result, once, then `ok`;
  - `bad-token`: drops a `{0}`.
- [ ] **Step 2: Write the failing tests** in `tests/drive.rs`, each with two invented batches:
  - `the_driver_translates_reviews_and_settles_every_batch`, in mode `ok`:
    - every batch has an answer, a review and, where the review replaced a line, a settled file;
    - `status` counts them;
    - the log shows `--tools ""`, the `cwd` outside the repository, and never more than two runs at once, using start and end stamps in the log;
  - `a_partial_or_junk_answer_is_never_kept`: modes `short`, `wrong-fp` and `junk` leave no `answers/<id>.json`, and after `retries`, `queue/<id>.json` names the lines;
  - `a_usage_limit_waits_and_resumes`: mode `limit` gives `waiting` once, then all done. No job is queued for it, and the wait is a test-sized `Duration`;
  - `a_restart_resumes_and_a_pause_stops_new_jobs`:
    - kill the driver mid-run, start again: the finished batches aren't run again, and no half-written file exists;
    - with `PAUSE` present, no job starts.
  - `a_line_failing_its_checks_is_retried_with_its_problems`: mode `bad-token` once gives a second prompt that names `MissingToken("{0}")`.
- [ ] **Step 3: Run them to verify they fail.** `cargo test -p fenji-translate --test drive`. Expected: compile errors.
- [ ] **Step 4: Implement** with `std::process::Command` and at most `runs` threads.
  - The CLI's result is JSON. A usage limit is the result the CLI gives for it; Step 6 records its exact shape. The wait is until the reset time it names, else 30 minutes.
  - Every answer is checked against its batch: the same fingerprints exactly once each, then `check` (Task 4) per line, under `Rules::load`.
- [ ] **Step 5: Run them to verify they pass.** `cargo test -p fenji-translate --test drive`. Expected: all pass.
- [ ] **Step 6: Make one real run, on one invented batch,** with the owner's subscription: `translate drive --only <id>`. Ledger:
  - the CLI's JSON shape;
  - where the structured answer sits;
  - the time per run.

  If the shape differs from the fake's, fix the parser and the fake together.
- [ ] **Step 7: Run the gate, then commit.** `feat(tools): the driver of headless runs, two at a time, resuming on its own`.

### Task 8: `translate assemble`, into a `fenji-content` checkout

**Files:**
- Create: `tools/translate/src/assemble.rs`
- Modify: `tools/translate/src/main.rs` (`assemble --to DIR`)

**Interfaces:**
- Consumes: Tasks 1, 3, 5, 6 and 7's files
- Produces: `pub fn assemble(work: &Path, lines: &[Line], to: &Path) -> Result<Coverage>`, where `pub struct Coverage { pub lines: usize, pub translated: usize, pub missing: Vec<Fingerprint> }`. It writes:
  - `to/text/en/<area>.tsv` with `fenji_pack::write_tsv`;
  - `to/glossary.tsv`;
  - `to/STYLE.md`, a copy of `prompts/style.md`;
  - `re/retranslation/coverage.txt`.

  A line's English is its settled form if there is one, else its review's, else its answer's.

- [ ] **Step 1: Write the failing tests:**
  - `assemble_writes_one_sorted_file_per_area_and_counts_coverage`;
  - `assemble_refuses_chinese_and_refuses_the_fenji_repository_or_re`: an English line holding CJK stops it, as does a `to` inside the repository that holds `CLAUDE.md`, or inside `re/`;
  - `the_settled_form_wins`;
  - `assembling_twice_gives_the_same_bytes`.
- [ ] **Step 2: Run them to verify they fail.** Then implement, run them to verify they pass, run the gate, and commit: `feat(tools): translate assemble, the new English into fenji-content's files`.

## Phase B: the client, the glossary and the early check (local)

### Task 9 (local, RE): how the client loads its locale cache

**Files:**
- Create: `docs/re/retranslation-locale.md`, with findings in the house style (#, Finding, Tag, Evidence), holding no game text
- Modify: `docs/README.md`

- [ ] **Step 1: Trace statically, in Ghidra and REA,** how the client opens `data/caches/locale_<lang>.new_cache`:
  - the locale script `data/core/share/locale.luac`'s load;
  - the engine's file read behind it;
  - whether a loose file under the copy's `data/` is ever read before the archives;
  - which language the launcher's start selects.

  Cite REA Evidence IDs, and the pinned exe's addresses.
- [ ] **Step 2: Decide the early check's loader, as a ruling:**
  - (a) a loose file in a sandbox-private copy, if the engine reads one;
  - (b) a sandbox-private copy of the archives with a stored record and a stored `_index`;
  - (c) a debugger swap of the read buffer, in the sandbox, with the AGT watchdog armed.

  Ledger it: `Task 9: Ruling: <mechanism> — <evidence> — <cost if wrong>`.
- [ ] **Step 3: Write the findings, then commit.** `docs(re): how the client loads its locale cache, for the retranslation's early check and the patcher's hook`.

### Task 10 (local): the loader for the early check

**Files:**
- Create:
  - `crates/fenji-data/src/cache/write.rs`: `pub fn write(cache: &Cache) -> Vec<u8>`, the pickle of payloads.md P.1–P.2 and m2-tables.md §2;
  - the loader that Task 9's ruling chose, under `scripts/spike/`;
  - `tools/translate/src/rebuild.rs`: `pub fn rebuild_locale(original: &[u8], english: &HashMap<Fingerprint, String>, added: &[(String, String)]) -> Result<Vec<u8>>`.
- Test: `crates/fenji-data/src/cache.rs` tests, `tools/translate/src/rebuild.rs` tests

- [ ] **Step 1: Write the failing tests:**
  - `a_cache_written_reads_back_the_same`: `read(&write(&c)) == c` for synthetic caches, with a shared table reference, a magic object and each scalar tag;
  - `rebuild_replaces_and_adds_by_fingerprint`: an invented locale cache with one line replaced and one added reads back with both.
- [ ] **Step 2: Run them to verify they fail.** Implement, then run them to verify they pass.
- [ ] **Step 3: Check against the real file, locally.** `read` and then `write` on `re/m2/tables/locale_en.new_cache` gives back the same bytes. If it doesn't, the difference is a finding, ledgered, and the writer is fixed.
- [ ] **Step 4: Build the loader,** and prove it in a sandboxed run (`scripts/spike/run-client.sh`, every guard). One invented English line replaces one known interface line, and shows on screen. Keep the screenshot under `re/retranslation/early/`.
- [ ] **Step 5: Run the gate, then commit.** `feat(data): the cache writer, and the early check's loader (sandbox only)`.

### Task 11 (interactive): the glossary, the sample and the early check

- [ ] **Step 1: Draft the glossary** from `translate glossary candidates`, in a session, as RT8 and RT9 say:
  - every `Name` with 3 or more uses;
  - every class, sect, place and title;
  - the recurring words of item names.

  Save it as `re/retranslation/glossary.json`.
- [ ] **Step 2: Show the owner the sample** in the conversation (RT15):
  - the glossary's 50 most used names;
  - one chapter of the main story, about 100 lines;
  - one screen of the interface.

  Each line sits beside today's English. Nothing is committed. Ledger the owner's word, and any changes the owner asks for.
- [ ] **Step 3: The early check (RT16).**
  - Translate about 300 lines in a session: the interface's tightest screens, one conversation, one skill's description and one book.
  - Load them with Task 10's loader, in sandboxed runs.
  - Look at screenshots, kept under `re/retranslation/early/`.
- [ ] **Step 4: Set the rules from what the check showed,** in `re/retranslation/rules.json`, which `Rules::load` reads, with each one a ruling in the ledger:
  - `Rules.budget_percent`, starting from 110;
  - `Rules.allowed`: the characters today's English uses, minus CJK, from `lines.jsonl`;
  - any token the client drew differently than expected.
- [ ] **Step 5: Freeze the glossary.** Ledger its size and its file's SHA-256. From now on, a change to the glossary reruns every batch that uses the term.

### Task 11b (interactive, with the bulk paused): the glossary translated afresh (the spec's Amendment A)

**Files:**
- Modify: `tools/translate/prompts/style.md` (§3), `tools/translate/src/prompt.rs` (its test), the spec (Amendment A)
- Create: `tools/translate/terms/plan_terms.py` and `assemble_terms.py`, with their test, `test-terms.py`, which CI runs (#506). They were first kept in the work folder, and lost with it.
- Local only: `re/retranslation-terms/`, the entries' work folder

- [ ] **Step 1: The rules** (#487). The style guide's §3 stops keeping the official names, and holds the genre's standard terms (RA1, RA2).
- [ ] **Step 2: The entries' batches** (RA3), written by `plan_terms.py`:
  - every entry of `re/retranslation/glossary.json`, by its kind and its family (a class with its skills, a family's people, a region's places);
  - in rounds by nesting: an entry holding no other in round 0, and one holding others in the round after theirs, with their new English in its batch's glossary;
  - each entry a line:
    - its Chinese;
    - today's English as `en_old`;
    - a context that says what it is, its old note's naming rules, how many lines hold it, and three of them in Chinese;
    - the budget of the tightest line that is the entry alone.

  The folder also holds `lines.jsonl` and `rules.json`, which the driver reads.
- [ ] **Step 3: Drive each round** with `translate --out re/retranslation-terms drive`, then plan the next one from the last one's English. The queue is read and settled in the session, as the bulk's is.
- [ ] **Step 4: Assemble** `re/retranslation/glossary.next.json` with `assemble_terms.py`:
  - each entry's settled English, with its kind;
  - its variants and note rebuilt (RA4).

  Review the most visible names in the session (RA5), and ledger the calls.
- [ ] **Step 5: The swap** (RA6):
  - the tool from `dev`;
  - the glossary re-frozen;
  - the bulk's batches replanned;
  - its finished batches moved aside to run again;
  - the bulk resumed.

  Ledger the glossary's size and its SHA-256.

## Phase C: `fenji-content`

### Task 12: the repository, its notice, its style guide and its releases

**Repository:** `theintonerzero/fenji-content`, public. Creating it is the owner's action, or needs the owner's go-ahead (§8).

**Files (in `fenji-content`):**
- `README.md`: what it holds, and how to report a wrong line (RT14);
- `NOTICE.md` (RT3);
- `STYLE.md`: Task 8's copy of `tools/translate/prompts/style.md`;
- `text/en/.gitkeep`;
- `scripts/pack.sh`: builds `fenji-content-<version>.tar.gz` with `manifest.json` in `fenji-pack`'s format (Task 1);
- `.github/workflows/release.yml`.

**Files (in Fenji):**
- `crates/fenji-pack/src/lib.rs`: `pub const CONTENT_PUBLIC_KEY: &str`, the owner's minisign public key;
- `docs/development.md`: "Releasing a content pack".

- [ ] **Step 1: Ask the owner for two things:**
  - the repository: create it, or approve that it be created;
  - the content key: `minisign -G -p content.pub -s content.key`. Its secret goes into `fenji-content`'s `release` environment as `CONTENT_SIGNING_KEY`, with `CONTENT_SIGNING_PASSWORD`, behind the owner's approval, as Fenji's release environment is.
- [ ] **Step 2: Write the files.**
  - `pack.sh` refuses any file holding CJK, and any entry `fenji_pack::read_pack` would refuse.
  - The workflow, on a `v*` tag:
    - runs `pack.sh`;
    - signs the pack with `minisign -S`, behind the release environment;
    - attaches the pack and its `.minisig`.
- [ ] **Step 3: Release a test.** Tag `v0.0.1` with five invented lines in `text/en/test.tsv`. Then check it in Fenji's repository: `fenji_pack::verify` and `read_pack` pass on the downloaded files, in a test that's ignored by default (`#[ignore]`, network). Then delete the release and its tag, and remove `test.tsv`.
- [ ] **Step 4: Commit in Fenji.** `feat(content): the content key, and how a content pack is released`.

## Phase D: the bulk

### Task 13 (local, long-running): every batch, then the pack

- [ ] **Step 1: Plan the batches.** `translate batch`. Ledger the batch count.
- [ ] **Step 2: Start the driver**, so it survives the session: `systemd-run --user --unit fenji-retranslation -- …/translate drive`.
  - The owner pauses it with `touch re/retranslation/PAUSE`, or with `systemctl --user stop fenji-retranslation`.
  - While it runs two, sessions dispatch no subagents.
- [ ] **Step 3: In sessions, work the queue,** in the spec's order (§6.5). Look at `translate status` and `re/retranslation/queue/`:
  - fix lines, or rule on them;
  - batch by batch, the interface first.
- [ ] **Step 4: When every batch is settled, assemble.** `translate assemble --to <fenji-content checkout>`. Expected: `coverage.txt` shows `missing: 0` of the lines players see.
- [ ] **Step 5: Run the checks over all of it.** Every line, through `check`, gives no problem. Ledger the totals.
- [ ] **Step 6: Open the translation's PR in `fenji-content`,** and release it there as `v1.0.0`, signed, with the owner's approval. It ships with the patcher, in 0.6.0 (D8). The final check in the game (§6.5, step 10) is in the patcher's plan, once the hook exists.

## Phase E: the launcher (lands with the patcher's hook, for 0.6.0)

### Task 14: the launcher fetches and checks the pack, and Settings' switch

**Files:**
- Create: `crates/fenji-launcher/src/content.rs`
- Modify:
  - `crates/fenji-launcher/src/settings.rs`: `#[serde(default = "yes")] pub new_english: bool`;
  - `crates/fenji-launcher/src/app/mod.rs`: `Intent::SetNewEnglish { on: bool }`;
  - `crates/fenji-launcher/src/app/snapshot.rs`: `SettingsView.new_english: Option<bool>`, which is `Some` on Linux;
  - `apps/launcher/src/popovers/settings.ts` and `settings.test.ts`;
  - the goldens under `apps/launcher/fixtures/snapshots/`, re-blessed;
  - `PRIVACY.md`, §6.3's request;
  - `docs/superpowers/specs/2026-10-15-m4-launcher-design.md`: a pointer under §8's amendments to the retranslation spec's §6.3. The plan's choice: the retranslation spec carries the content pack, and the launcher spec points to it.

**Interfaces:**
- Consumes: `fenji_pack::{read_pack, verify, CONTENT_PUBLIC_KEY, Pack}` (Task 1)
- Produces:
  - `pub const CONTENT_RELEASES: &str = "https://api.github.com/repos/theintonerzero/fenji-content/releases/latest"`;
  - `pub struct ContentRelease { pub version: String, pub pack: Url, pub signature: Url }`;
  - `pub async fn latest(api: &str) -> Result<ContentRelease, ContentError>`;
  - `pub fn installed(dir: &Path) -> Option<(String, PathBuf)>`;
  - `pub async fn ensure(dir: &Path, release: &ContentRelease, pinned_build: &str, progress: &(dyn Fn(u64, Option<u64>) + Send + Sync), stop: &proton::Stop) -> Result<PathBuf, ContentError>`, with `proton::ensure`'s shape:
    - the release's pack is fetched, verified against `CONTENT_PUBLIC_KEY`, read with `read_pack`, and refused unless its `manifest.build` is `pinned_build` (`AppConfig.pinned_build`);
    - it is kept as `dir/<version>.tar.gz`, beside its `.minisig`, and older packs are removed;
    - a stop leaves nothing half-made.
  - The patcher's plan wires `ensure` into Play, and hands the `Pack` to its hook.

- [ ] **Step 1: Write the failing tests,** mirroring `proton.rs`'s, against a local HTTP server and an invented pack signed with a test key:
  - `a_newer_pack_is_fetched_checked_and_kept`;
  - `a_bad_signature_or_another_build_is_refused_and_the_pack_there_kept`;
  - `offline_the_pack_there_is_used`;
  - `a_stop_leaves_nothing_half_made`;
  - `the_new_english_switch_is_on_by_default_and_saved`, as `gamemodes_switch_…` is;
  - the view's `settings.test.ts`: the switch sends `{ type: "setNewEnglish", on: false }`.
- [ ] **Step 2: Run them to verify they fail.** Then implement, run them to verify they pass, re-bless the goldens (`FENJI_BLESS=1`), and run the launcher's own checks.
- [ ] **Step 3: Commit, after the gate.** `feat(launcher): the content pack, fetched and checked, and the new English's switch`. This PR merges with the patcher's hook, for 0.6.0.
