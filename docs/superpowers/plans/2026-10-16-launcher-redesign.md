# The launcher's redesign: Implementation Plan

> **For agentic workers:** built inline by the controller, as the launcher was (the owner, 2026-10-15 and 2026-10-16), under superpowers:executing-plans. Test-first for behaviour. The UI follows impeccable: its craft floor before each UI edit; its detector, finish reviewer and documenter at the end.

**Goal:** rebuild the launcher's view as the approved boards, on the same model and intents. The model gets two changes (R5, R8), and the Game popover gets the QR code once the follow-ups plan's Task 11 rehearsal lands.

**Spec:** `docs/superpowers/specs/2026-10-16-launcher-redesign-design.md` (rulings R1–R10). **Boards:** `docs/design/launcher/redesign/screens.html`. **Direction contract:** `.impeccable/surfaces/apps-launcher.md`.

## Global Constraints

- **The boards and the spec's rulings are binding.** DESIGN.md's tokens stay the palette, the type and the radii until the documenter rewrites DESIGN.md at the finish. R6's 64 px name is the one change.
- **The window:** 1200 × 720, close and minimise only, with the top bar as its drag region. The content security policy stays `default-src 'self'`: fonts and assets ship inside the app.
- **Safety:**
  - no credential ever reaches the view;
  - no game art;
  - players' and hosts' text is shown as text, never parsed as HTML (the existing `check-no-html.mjs` guard).
- **The gate before the PR:**
  - `apps/launcher`: `pnpm run check`, `pnpm test` and `pnpm run build`;
  - `apps/launcher/src-tauri`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`;
  - for the model's changes, the workspace gate (`cargo fmt`, `clippy`, `test` and `deny`, and the scripts).
- **Commits:** Conventional, scope `launcher` (the plan's own: `docs`). Each ends with one `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` line, and never carries a session link.

## Tasks

### Task 1: the model's two changes (R5, R8)
- **R8:** `servers::run` probes every server every 30 s, and the active one every 15 s, whatever the view shows.
  - A test shows the list's probes coming with no `ViewTab { tab: servers }`, and fails today.
  - `ViewTab` stays in the model for compatibility; the new view never sends it.
- **R5:** `app/words.rs`'s sentences are shortened to the glossary, with their tests and the golden snapshots updated.

### Task 2: the shell: the rail, the top bar and the stage
- **Styles:**
  - the tokens: the 64 px name, and the popover's 20 px title;
  - the rail: seals, state bars, +, and a seal's tip on hover or focus;
  - the top bar: the address, the Game chip, the avatar, the gear and the window controls, as the drag region;
  - the stage: a warm, cool or plain ground, and the centred group.
- **The route** (`route.ts`): welcome, create your account, sign in, or the stage. Popovers are the view's own state.
- **What goes:**
  - the header and its tabs;
  - the context column;
  - the blade;
  - the Servers screen;
  - the Client tab's screens, whose content moves into the Game popover.
- **Tests:**
  - the route;
  - the rail: state bars, the active seal, choosing a server (`selectServer`), and the keyboard;
  - the top bar's controls.

### Task 3: PLAY kindles, and the stage's states
- **PLAY** (`ui/play`):
  - four states: lit, kindling (its progress and its label), closed and running;
  - a progress role with its value;
  - reduced motion: no glow on the edge.
- **The stage:**
  - its states: ready, verifying, starting, running, maintenance, offline, key changed and unsupported;
  - at most one secondary action;
  - the meta line, only when needed;
  - the friends.
- **Tests per state:** its words, at most one ember, and the intents it sends.

### Task 4: getting in
- Welcome, create your account and sign in, as task screens.
- Add a server, as a dialog.
- The changed-key dialog, restyled.
- Tests for each.

### Task 5: the popovers and the toast
- **Game:**
  - verified, with Verify integrity;
  - set up: the Steam library first, then another folder, then download;
  - downloading: the terminal path until Task 6;
  - needs repair.
- **Account and Settings.**
- **The update toast.**
- **How a popover behaves:**
  - it's anchored under its control, with that control shown open;
  - it sits over a scrim;
  - Esc or the scrim closes it;
  - focus moves in when it opens, and back to its control when it closes.
- Tests for each.

### Task 6 (after the follow-ups plan's Task 11): DepotDownloader inside the launcher
The follow-ups plan's Task 12, with its code drawn in the Game popover's Downloading state (R9).

### Task 7: the finish
1. The hostile-text test runs over every new screen and popover.
2. Impeccable's detector runs on the changed view.
3. Screenshots at 1200 × 720, one per board, go into `.impeccable/review/`.
4. Impeccable's finish reviewer reviews them against the boards and the direction contract, then its fixes are made.
5. Impeccable's documenter rewrites DESIGN.md (R10).
6. One final code review of the branch.
7. The PR, then the release, with the follow-ups.
