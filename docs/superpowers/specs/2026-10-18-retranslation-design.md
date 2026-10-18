# Retranslation: the game's English, translated anew from the Chinese — Design Spec

**Status:** written on 2026-10-18 (UTC) from a conversation with the owner. The owner decided what it is, why, where its text comes from, who gets it and where it lives (§4), and then said "do all as you recommend". After a reviewer's check, the owner also chose the subscription for the work and 0.6.0 for its release (D7, D8). Every other choice is a ruling, RT1 to RT16 in §9, with what it costs if it's wrong, so the owner can reverse any of them. It answers gate #338's three questions (§4.2) and amends ground rule 3 (§4.3). It needs two things from the patcher (#336), which gets its own spec next (§8).

## 1. Purpose

Swords of Legends Online's English is what players complained about most. The owner said so after entering the world on bare metal (2026-10-14): "the english translations are very poor, one thing we need to do in the future is translate all content in the game to english better. Like a proper translation". And again on 2026-10-18: "Every single thing makes me wince with the translation even if you search the web the largest complaint was localisation". Reviews agree. PC Gamer's review found the story's English broken badly enough that the reviewer stopped following the plot, and called the interface jumbled. On Steam, players reported characters switching to Chinese mid-conversation.

The work is done when a player who plays in English, on any Fenji server, reads a new translation everywhere in the game. It is made from the original Chinese, "A full professional translation, improved grammar etc" (the owner). It has one consistent voice, gives every name the same way everywhere, and leaves nothing in Chinese.

## 2. Goals, non-goals and anti-goals

**Goals:**
- **Everything players read,** translated anew from the Chinese, with today's English beside it as context: "Re translate from the Chinese and look at the English too for max context" (the owner). That covers:
  - quests and dialogue;
  - items, skills and NPCs;
  - books and lore;
  - the interface and system messages.
- **Every line in English.** That includes the lines that have no English today (§5).
- **The voice comes from the Chinese,** speaker by speaker: "I think you should infer how it should sound from the chinese" (the owner).
- **American spelling,** as the owner chose.
- **Everyone gets it:** "everyone playing on any Fenji servers who needs english" (the owner).
- **The mechanics stay exact.** Markup, placeholders and numbers come through unchanged, so a skill or item still says precisely what it does.
- **It fits the interface.**

**Non-goals:**
- **The voice-over.** It stays as it is, because this work is text only. The new English pairs best with the Chinese voices, which the game's settings offer, since the English voice-over follows a different script.
- **Text drawn into images.** If the builder finds any that matters, it is listed for later (§10).
- **Other languages.** They can come later, in the same repository and the same form.
- **The game's numbers, mechanics or balance.**
- **The text Fenji's server sends itself.** It is already Fenji's own.

**Anti-goals,** which would make this a failure even if it worked:
- a client that crashes or draws garbage because markup or a placeholder broke;
- a name or term that reads two ways;
- new and old English mixed, reading worse than either;
- any of the game's own text, its Chinese or its original English, in a repository;
- a patched client that could reach anything but Fenji.

## 3. Constraints

- **The ground rules hold, rule 3 as amended in §4.3:**
  - **Rule 1:** each player's own copy. Nothing of the game's own ships: no file, no Chinese line and no line of its original English.
  - **Rule 2:** the clean room holds. Nothing comes from leaked server files or unofficial servers, including any translation they carry.
  - **Rule 4:** a client whose text has changed is a patched client, so it only ever talks to Fenji. The patcher's "every way out" set ships with this work or before it (#336).
  - **Rule 6:** the change is applied in memory at launch, never by changing the game's files. The decision gates stay: the AGT driver, any service but Fenji's, and Oodle outside its licence.
- **What comes from the copy stays local.** The tools read the Chinese text and today's English from a copy of the game on the machine doing the work, the game box. They keep everything derived from it in a git-ignored work folder, `re/retranslation/`. None of it goes to cloud agents, by the owner's rule for cloud credits, or into any repository.
- **The pinned build.** The translation is made from build 10015727 (`docs/re/pinned-build.md`) and keyed to its lines.
- **Clean-room hygiene** applies as everywhere: reading the client's text is RE work.

## 4. The owner's decisions (2026-10-18)

### 4.1 What the owner decided

| # | Decision | The owner's words |
|---|---|---|
| D1 | Translate from the Chinese, with today's English as context. | "Re translate from the Chinese and look at the English too for max context" |
| D2 | The voice comes from the Chinese. | "I think you should infer how it should sound from the chinese" |
| D3 | American spelling. | "do american english spelling please" |
| D4 | Everyone on any Fenji server who plays in English gets it. | "everyone playing on any Fenji servers who needs english" |
| D5 | The translation's files may be in a repository: a separate one, `fenji-<something>`, for all the content Fenji ships that changes the game. | "its okay to have the files for the translations in the repo"; "you can do it within like fenji-<somthing>"; "not just translations, for any other files we ship that modify the cotent of the game no? so fenji-<something> else" |
| D6 | The rest are Claude's rulings (§9). | "do all as you recommend" |
| D7 | The work runs on the owner's Claude subscription, not the API (§6.5). | "do as you recommend, use my subscription." |
| D8 | It ships in 0.6.0 with the patcher, after M4 closes with its acceptance session's fixes. M5's slices follow as 0.7 and on, to 1.0.0 (§8). | The same answer, to Claude's recommendation of a release of its own. |

The owner also confirmed three guesses, in the same conversation:
- the work is text only;
- every line comes out in English;
- it is applied in memory at launch, with "every way out" shipping with it or before it.

### 4.2 Gate #338's three questions

1. **Where the text comes from:** from the pinned build's own Chinese and English, read from a copy of the game on the machine doing the work (D1). Fenji ships neither.
2. **How a player applies it:** they don't. The launcher fetches the content pack, and the patcher applies it in memory at launch (§6.3, §6.4). The player's files never change.
3. **What may be shared:** the new English, in the public repository `fenji-content`, keyed by fingerprints of the game's lines (D5, §6.1, §6.2).

### 4.3 Ground rule 3, amended

The rule keeps its list of what never enters Fenji's repository. The amendment adds one sentence, which goes into `CLAUDE.md` with this spec:

> The one place for content Fenji makes that changes the game is the separate repository `fenji-content` (the owner, 2026-10-18, #338): Fenji's own new content only, such as the retranslation's English, keyed by fingerprints of the game's lines, never the game's own files or text.

## 5. What we know

From [`docs/re/m2-tables.md`](../../re/m2-tables.md) and a count over the pinned build's decoded caches (2026-10-18 UTC). The count gave totals only; no text was printed.

- **The text's source language is Chinese.** The tables keep their text as `LString`s: 354,148 of `all_tables`' 354,404 are `L("…")` around the Chinese (m2-tables 2.6), 122,800 of them distinct.
- **The English is one map.** The locale cache `locale_en` maps each source line to its English, with 175,415 entries in its main table (2.6). Caches for `de`, `fr`, `kr`, `ru_RU`, `vi` and `zh_MYSG` exist beside it (1.2).
- **The scripts' own text** probably goes through the same map (Inferred, not traced). The `L("…")` form is the locale script's, and the builder confirms where the scripts' strings go (§10).
- **Size:**
  - the sources hold 3.0 million Chinese characters, and 6.8 million characters with markup, digits and spaces;
  - today's English runs to 2.5 million words (17.1 million characters);
  - half the lines are 9 characters or fewer, and nine in ten are 64 or fewer; one in a hundred is longer than 670, and the longest is 4,639.
- **Markup in the sources,** counted as lines holding each:
  - `[`: 22,843;
  - a line break: 12,404;
  - `{`: 4,396;
  - `#`: 3,201;
  - `%`: 2,530;
  - `|`: 2,247;
  - `<`: 1,750.

  The tokens' shapes, with digits as 9 and Chinese as 中, show at least four families. Their full grammar is the builder's to establish (§10).
  - **Bracket tags, like BBCode:**
    - colour, `[c 99ccff]` … `[/c]` (24,391 closing tags);
    - `[size 99]`, `[leading 99]`, `[indent]`, `[center]`;
    - links and references, `[skill:9999999]`, `[q_guid value='99999999']`;
    - names and speakers that hold visible text, `[q_npcname text="中"]` (16,344) and `[d_npc name="中" action=""]`;
    - the player's own name, `[rolename]`.
  - **Brace placeholders, in Python's format style:** `{9}` (6,988), `{9:d}`, `{9:.9f}`. Some are named, `{target}` and `{damage}`, and a few hold visible text, `{color 99ff99 中}`.
  - **HTML-like tags:** `<font color="#…">` … `</font>` (2,001 closing tags), `<br>`, `<b>`, links.
  - **Codes in skill and effect text,** between pipes and hashes: `|cp,…`, `|dam…`, `|end`, `#中#`. They are followed by units such as `%`, which look like the game's computed values.

  So some markup carries text players see (a name inside a tag, a word inside a colour code), and that text is translated. The markup around it is kept.
- **Gaps today:**
  - about 9,100 distinct table lines have no entry in `locale_en` at all. That count is approximate: it unescaped Lua strings simply;
  - 1,107 entries give the source itself as the English;
  - 1,031 English entries still hold Chinese characters.

  These are the lines players see in Chinese.
- **The decoding exists:**
  - `fenji-data`'s cache reader, and `archive extract` for the caches (m1-archives.md; m2-tables.md 1.6);
  - `fenji-oodle`, Fenji's own open Oodle decoder ([`2026-10-14-oodle-decoder-design.md`](2026-10-14-oodle-decoder-design.md)). It reads every Oodle block of the archives as the client's own library does, so Fenji's tools and the launcher can read a player's caches without Epic's library (rule 6).

## 6. The design

### 6.1 Where it lives

- **`fenji-content`** is a new public repository beside Fenji's (RT1). It holds everything Fenji ships that changes the game's content, and the retranslation is its first part, in `text/en/`. It holds:
  - the new English, each line keyed by its fingerprint (§6.2);
  - the glossary (§6.5): each term's fingerprint, its English and a note in English;
  - the style guide (§6.6), which holds no game text;
  - a notice (RT3). The files translate the game's text, so they aren't Fenji's to put under the AGPL. They are for players of their own copies, and the game's text belongs to its rights holders.
- **Fenji's repository** holds the tools that make the content, and the launcher's code that fetches and checks it. Neither holds game text. Their tests use invented lines.
- **Releases.** `fenji-content` is released by tag. Its CI builds the pack from its files and signs it (§6.3).

### 6.2 Fingerprints and the pack

- **A line's fingerprint** is the first 16 bytes of the SHA-256 of its exact source bytes, as the locale map holds them (UTF-8), written as 32 hex characters (RT2). At this size, 128 bits leave no chance of two lines colliding.
- **The files** (RT2):
  - one entry per line: `<fingerprint>` TAB `<English>`, with tabs, line breaks and backslashes escaped;
  - split by area (interface, items, skills, quests, dialogue, lore and so on), so that reviews and diffs stay readable;
  - sorted by fingerprint within each file.
- **The pack** is every file of `text/en/` in one compressed file, with a manifest: the build it's for (10015727), the content version, its line count and its SHA-256.
- **A line the pack lacks** keeps the game's own English.

### 6.3 How players get it

- **The launcher** (Linux, where Play is today):
  - asks for `fenji-content`'s latest release before each start, as it does for GE-Proton (launcher spec LB1, RT6);
  - fetches a newer pack;
  - checks the pack's signature against a key it carries, a content key the owner makes as with the release key (hosting spec HD5, RT4);
  - keeps the pack in its data folder. Offline, the pack it has plays.
- **Settings** has a switch for the new English, on by default (RT5). Turned off, the game shows its own English.
- **PRIVACY.md** lists the new request: `fenji-content`'s latest release, from GitHub.

### 6.4 How the client shows it: what it needs from the patcher

- **The patcher's content hook** (#336) gives the client its own locale map in memory at launch, with the pack's English for every line the pack has. The map is built from two things:
  - the player's own copy: its Chinese keys, and today's English for lines the pack lacks;
  - the pack, matched by fingerprint.

  Lines the client has no English for at all, about 9,100 (§5), get an entry too. Their Chinese comes from the player's own tables.
- **How it does that** is the patcher spec's to choose, after RE: for instance by serving the rebuilt `locale_en` where the client reads it, or by replacing the loaded map's values. The player's files never change.
- **Every way out first.** The pack only reaches the client with the patcher's "every way out" set in place (rule 4).
- **Another build** still works: a copy of another build keeps its own English wherever its lines differ from the pinned build's.

### 6.5 How it's made

- **Who** (RT7): Claude, on the owner's Claude subscription (D7), on the game box, where the copy and `re/` are.
  - **A driver** on the game box runs the batches unattended. It starts headless Claude Code runs, at most two at a time by the owner's rule, on the most capable model.
  - **The runs get no tools.** Each is handed one batch, with the style guide and its glossary terms, and answers with the batch's English. It reads nothing else, and writes nothing.
  - **The driver keeps the answer** once it passes the automatic checks (§6.7). It resumes where it stopped after a crash or a restart, and waits out the subscription's usage limits.
  - **The owner can pause it** at any time.
  - **Interactive sessions** do the glossary, the sample, the early check, and whatever the checks or the reviews send back.

  The batches are plain files, so the same work could go through the API if the owner later wants it faster.
- **The work folder** is `re/retranslation/`, which is git-ignored. It holds everything derived from the copy. The project's ledger (`.superpowers/sdd/<plan>/progress.md`) keeps the place between sessions.
- **The steps:**
  1. **Extract.** For each line, take from the copy:
     - its Chinese, today's English and its fingerprint;
     - its markup tokens;
     - where it's used: the table, field and row; the script; and the quest, NPC and order of a conversation, where the tables give them.

     Also find every text source outside the locale map, and record what this work covers (§10).
  2. **Glossary.** Names and recurring terms come first: characters, places, sects, classes, skills, titles, and the recurring words in items' names. Each gets one English form, with a note (RT9):
     - personal names in pinyin;
     - the official English kept where it's right and consistent;
     - a corrected form where it isn't.

     The glossary is frozen before the bulk of the work (RT8). A later change updates every line that uses the term.
  3. **A sample for the owner** (RT15): the glossary's main names, one chapter of the main story and one screen of the interface, each beside today's English, shown in the conversation and never committed. The bulk waits for the owner's word on the voice.
  4. **An early check in the game** (RT16). Before the bulk, a few hundred lines are loaded into a sandboxed client by a loader for development only, which never ships. Rule 4 holds, because a sandboxed run talks only to Fenji. They are lines from the interface's tightest screens, a conversation, a skill's description and a book. The check confirms:
     - the markup's grammar and the escapes;
     - the font's characters;
     - the interface's room.

     The length budget (RT11) is set from it. The loader shares its RE with the patcher's content hook (§6.4).
  5. **Batches.** Lines are grouped by context, sized to fit one agent's context with the glossary's terms for them. A group is, for instance:
     - a quest's lines;
     - a conversation, in order;
     - a family of items;
     - one screen of the interface.
  6. **Translate.** Each batch is translated from the Chinese, with today's English and the context beside it, following the style guide (§6.6).
  7. **Check automatically** (§6.7).
  8. **Review** (RT13). A second agent reads each batch against the Chinese for accuracy, voice and consistency, and fixes what it finds. Where the two disagree, a third reading settles it.
  9. **Assemble** the batches into `fenji-content`'s files, by area.
  10. **Check in the game,** once more at the end, through the patcher's content hook. In guarded client runs, sandboxed (rule 4), with the whole pack applied, look at:
     - the interface's tightest screens;
     - a quest and a conversation;
     - a book.
- **Order.** The glossary first, then:
  1. the interface and system text;
  2. items, skills and the other game data;
  3. the main story;
  4. side quests and dialogue;
  5. lore and the rest.

  It ships whole (RT12).
- **Scale.** About 3 million Chinese characters become some 2.5 million words of English, and the review reads them all again. That's roughly 60 million tokens in and 6 million out. With two runs at a time, inside the subscription's limits, it takes weeks.

### 6.6 The style guide

The style guide is a file in `fenji-content`. It holds no game text, and its examples are invented. It says:
- **The voice comes from the Chinese,** speaker by speaker (D2):
  - formal, classical speech becomes formal, slightly old-fashioned English, never "thee and thou" pastiche;
  - casual speech becomes casual English;
  - jokes and wordplay are translated for their effect;
  - poems are rendered as verse.
- **American spelling and punctuation** (D3).
- **Names and terms come from the glossary** only.
- **Markup, placeholders, line breaks and numbers** stay exactly as they are in the Chinese, in the order the markup's grammar needs. Text inside markup that players see is translated, through the glossary: a name in a tag's attribute, or a word inside a colour code (§5).
- **Only characters the game's English font draws** (RT10). Today's English uses printable ASCII, line breaks and tabs, and a few others: the en dash, the no-break space, `…`, `·`, the em dash, a few accented letters and Roman numerals. The builder lists them, leaving out the Chinese of untranslated entries, and the new English uses no others. So there are no curly quotes unless the list holds them.
- **Length.** Where the interface is tight (buttons, tabs, item and skill names, short labels), a line stays close to today's English length, within a budget the builder sets after the check in the game (RT11).
- **Faithful, not copied.** Today's English is context, and the new text comes from the Chinese.

### 6.7 Checks

- **Automatic, on every line:**
  - markup and placeholder tokens are kept;
  - no Chinese characters are left;
  - only the allowed characters are used;
  - glossary terms are written as the glossary has them;
  - lines within the length budget, where it applies;
  - American spelling, against a word list;
  - nothing is empty;
  - each fingerprint has one translation.
- **Coverage:** every line of the pinned build that players can see has a line in the pack.
- **The review** (§6.5, step 8), and **the checks in the game**, early (step 4) and at the end (step 10).

## 7. Fenji's other changes

- **CLAUDE.md:**
  - rule 3's sentence (§4.3);
  - rule 6's "starts after M4", which now says the patcher ships in 0.6.0 and M4's releases keep the unmodified client;
  - the versions (D8) and the status line.
- **`docs/development.md`:** the versions (D8), with this spec. Later, how to build, sign and release a content pack.
- **GitHub:** a milestone for 0.6.0, holding #336 and #338.
- **PRIVACY.md:** the content fetch (§6.3).
- **The launcher spec:** Settings' switch and the content pack, as an amendment there or as this spec's own section, as the plan chooses.
- **`docs/re/m4-summary.md`:** the patcher, which it lists among M5's inheritances, ships in 0.6.0.
- **`docs/README.md`:** this spec.
- **Gate #338:** the owner's answers (§4.2), then closed.

## 8. Dependencies and order

- **The patcher** (#336) gets its own spec next. It covers the content hook this work needs (§6.4) and its "every way out" set, with the AGT driver and a Steam copy's needs from the owner's first patch set. The retranslation ships when both the pack and the patcher are ready, in 0.6.0 (D8).
- **M4 closes first,** with its acceptance session and whatever that finds, in 0.5.x releases that keep the unmodified client. M5's slices follow 0.6.0, as 0.7 and on, up to 1.0.0.
- **The translation work** (§6.5) starts at once. The extractor, the glossary, the sample, the early check's loader and the batches don't need the patcher. The loader's RE serves the patcher's hook too.
- **`fenji-content`** is created when its first files are ready. Creating a repository on the owner's account, and the content key, are the owner's actions, or need the owner's go-ahead.

## 9. Rulings

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| RT1 | The repository is `fenji-content` (§6.1). | The owner's "fenji-<something>"; the name says what it holds: all of Fenji's changes to the game's content. | A rename. |
| RT2 | Only the new English, keyed by 128-bit fingerprints of each line's exact Chinese; files split by area and sorted (§6.2). | No Chinese or original English in any repository (rule 3 as amended); readable diffs; no collisions at this size. | A format change, before the first release only. |
| RT3 | `fenji-content` carries its own notice, not the AGPL (§6.1). | The text translates the game's; a licence for Fenji's code can't cover it. | The notice's words. |
| RT4 | A content key of its own signs the pack (§6.3). | A content key can't sign a launcher or a server's release; the owner makes it as with the release key. | One more key to keep. |
| RT5 | On by default, with a switch in Settings (§6.3). | Everyone who plays in English should have it (D4); the switch gives back the original. | A default flipped. |
| RT6 | Fetched before each start, as GE-Proton is (§6.3). | Fixes reach everyone. | A pack a start behind. |
| RT7 | Claude translates on the owner's subscription (D7), through a driver of headless runs on the game box: two at a time, no tools, one batch each, resuming on its own (§6.5). | The owner's choice. Unattended, it keeps going between sessions. With no tools, a run can't read or touch anything but its batch. | Weeks of the subscription's usage, shared with the owner's own sessions. The owner can pause it, and the API route is there for speed. |
| RT8 | The glossary comes first and is frozen (§6.5). | Every name the same everywhere. | Lines to redo if a name changes. |
| RT9 | Personal names in pinyin; the official English kept where it's right (§6.5). | Players know the official names; pinyin is how Chinese names are written in English. | A renaming pass, through the glossary. |
| RT10 | Only the characters today's English uses (§6.6). | The font draws them; others may show as boxes. | Plainer punctuation. |
| RT11 | A length budget for tight interface text (§6.6). | The jumbled interface. | Terser wording where space is tight. |
| RT12 | It ships whole, not in waves (§6.5). | New and old English mixed reads worse, with names two ways. | Players wait for all of it. |
| RT13 | A second agent reviews every batch (§6.5). | A professional standard: translation, then review. | Twice the work. |
| RT14 | Players report a wrong line as an issue on `fenji-content`. | Where the text lives. | Another channel later. |
| RT15 | The owner reads a sample before the bulk (§6.5). | The owner's ear on the voice, before 2.5 million words. | A pause of a day or so. |
| RT16 | An early check in a sandboxed client, through a loader for development only, before the bulk (§6.5, step 4). | A mistake in the markup, the escapes, the font or the room would otherwise repeat across every batch, and show only at the end. | RE that overlaps the patcher's hook, done a little earlier. |

## 10. Left to the builder

- **The markup's grammar,** from the data and the client's text drawing, and the exact rules for its tokens.
- **Where else text lives,** and what of it this work covers:
  - the `data/tables/locale` `.tab` files;
  - the program's own strings;
  - the scripts' strings, if any skip the map;
  - text drawn into images, listed for later.
- **How to tell interface text from prose,** by where each line is used, and the length budget's figure.
- **The batches:** their sizes and grouping.
- **The tools' shape:** a crate, or `fenji-data` commands, in Rust like the rest of the workspace.
- **The pack's layout** within `text/en/`, and its compression.
- **The launcher's code** for the pack, and Settings' words (DESIGN.md's components).
- **The content hook's mechanism,** for the patcher's spec.
