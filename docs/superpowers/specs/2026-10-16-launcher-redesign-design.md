# The launcher's redesign: one stage, PLAY kindles

**Date:** 2026-10-16. **Status:** approved by the owner in the visual companion ("that's now nice and modern"), to be built inline (the owner: "build it all inline so we can ship it out").

**Supersedes:** the desktop launcher spec's screens and look (`2026-10-15-m4-launcher-design.md` §3.2, §3.3's Servers screen, §3.10 and the boards in `docs/design/launcher/boards/`). Everything else in that spec stays: the model, the intents, accounts, downloads, Play, updates, the window and the shell. Its Amendment A (Steam copies, DepotDownloader inside the launcher) is built into these screens.

**The approved screens:** `docs/design/launcher/redesign/screens.html`, 18 screens at 1200 × 720. They are the boards for this build, and their words are the launcher's copy. The direction contract is in `.impeccable/surfaces/apps-launcher.md`.

## 1. The owner's direction

The owner, on the launcher as built for v0.5.0: "it feels pretty clunky to be honest. more simple, streamline, more modern looking... but still with character". The owner's choices, in order:
1. Keep the identity, rebuild the layout. The ember-on-ink palette, gold for people, the 焚 seal and the type stay.
2. What felt clunkiest: too many tabs and screens, the side column everywhere, too much text.
3. Structure A: one stage, with the servers in a slim rail. B, a server switcher on top, and C, two trimmed tabs, were not chosen.
4. "lets remove the sword". The signature move becomes **PLAY kindles**, chosen over a living ember glow and the 焚寂 mark at large.
5. Avoid every one of these: plain or generic, busy, flashy or "gamer", and Discord-like.
6. "use more concise language in the interface": "Verify integrity", not "Check every file", and no file counts or build numbers.

## 2. Rulings

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| R1 | **One stage, no tabs and no side column.** The window is a 72 px rail of servers and a stage for the chosen one. Game, Account and Settings open as popovers under their own control in the top bar. | The owner's structure A. It answers "too many tabs and screens" and "side column everywhere". | Tabs back, as option C. |
| R2 | **Servers are square ink seals,** each with a 3 px state bar beneath: jade online, amber maintenance, faint offline or unknown, cinnabar key changed or incompatible. The active seal is ember-edged, and + adds a server. Hovering or focusing a seal shows its name and players. | A launcher's own vocabulary, not Discord's round avatars and presence dots. | Another shape for the rail. |
| R3 | **PLAY kindles.** The signature move, and the only ember motion: PLAY has four states. The stage's ground warms from the lower left when PLAY is lit, is cool while it kindles or a task runs, and stays plain ink when Play can't happen.<br>**Lit:** ember, ready.<br>**Kindling:** ink, with an ember fill to the progress and a soft edge, labelled "Verifying 78%" or "Starting".<br>**Closed:** ink, faint, when Play can't happen.<br>**Running:** an ember outline, a jade dot and "Running". | The owner's pick. Readiness is felt on the button the player came to press, as the sword's blade was before. | A plain progress bar. |
| R4 | **One line of state, one action.**<br>The state line reads "Online · 3/20 players · 24 ms", "Maintenance · back around 21:00", "Offline · last seen yesterday 23:10" or "Key changed".<br>The host's own notice gets one line, prefixed with their name in gold.<br>Beside PLAY there is at most one secondary action: Stop, Cancel, Refresh, Retry or Paste new link.<br>Friends follow PLAY: their faces and "Kai, Wen, Shu playing". | "Too much text", and the owner's concise-language rule. | Longer words back where a test shows players confused. |
| R5 | **The words are the boards' words**, through this glossary:<br>Verify integrity (the full check); Verifying; Verified; Repair; Game; Online, Maintenance, Offline and Key changed; Sign in and Sign out; Create account; Add.<br>No file counts or build numbers on screen.<br>The model's own sentences in `app/words.rs` are shortened to match. | The owner's concise-language rule. | Strings. |
| R6 | **The stage's group centres vertically** at x 144. The server's name is Cinzel at 64 px, up from DESIGN.md's 54. | The board as approved: the name owns the stage the sword used to fill. | One size. |
| R7 | **Popovers:**<br>404 px wide, anchored under their control, sized to their content.<br>A light scrim covers the stage, while the top bar stays above it with its control shown open.<br>Esc and the scrim close a popover. Focus moves in when it opens and back to its control when it closes.<br>Add a server, and a changed key, stay centred dialogs. An update is a corner toast. | Modern, compact and keyboard-whole. A full-height sheet was mostly empty space (the boards' first round). | Sheets back. |
| R8 | **The model probes every server every 30 s**, and the active one every 15 s, since the rail always shows the list. | The rail's state bars must be true without a Servers tab to open. | Probe traffic for a list of a few servers. |
| R9 | **DepotDownloader inside the launcher** (Amendment A, LA4 and LA5) is built here: the Game popover's Downloading state shows the QR code. It waits on the game-box rehearsal (the follow-ups plan's Task 11). Until then, the terminal path stays as it is. | The board shows it, and the owner asked for the Steam work in this build. | The terminal for everyone, as today. |
| R10 | **DESIGN.md is rewritten at the finish** from the built screens, by impeccable's documenter. The old boards stay in the repository as the v0.5 record, and the redesign's boards join them. | Impeccable's rule for a replaced world: document what was built, not what was meant. | A rulebook written early. |

## 3. Screens

The boards hold every screen, with real data and words:
1. **Playing:** ready, verifying, starting, running.
2. **When you can't play yet:** maintenance, offline, key changed, Windows or macOS.
3. **Getting in:** welcome, create your account, sign in, add a server.
4. **Game popover:** verified, set up (the Steam library first, then another folder, then download), downloading with the QR code, needs repair.
5. **Account popover, Settings popover, and the update toast.**

The route (`route.ts`) becomes:
- no server: welcome;
- a pending invite: create your account;
- a sign-in Play needs: sign in;
- otherwise, the stage for the active server, in the PLAY state that `playVariant` gives.

Popovers are the view's own state, over any stage.

## 4. Testing

- **View tests** cover each board's screen: its words, its one ember at most, its controls and the intents they send.
- **The hostile-text test** runs over every new screen and popover.
- **Keyboard:** the rail's seals, opening and closing each popover with focus returned, and Esc.
- **The model:** the list's probe cadence, with a test that would have failed before, and the shortened words' tests.
- **The finish:**
  - impeccable's detector on the changed view;
  - impeccable's finish reviewer, against the boards and the direction contract;
  - impeccable's documenter for DESIGN.md;
  - one final code review of the whole branch, as the launcher was built.

## 5. Out of scope

- Windows play (gate #337).
- Steam's Proton (Amendment A, LA6, the follow-ups plan's Task 13), which waits for its rehearsal.
- A resizable window. The window stays 1200 × 720, close and minimise only.
- Anything in the model beyond R5 and R8.
