---
name: Fenji
description: "One stage, PLAY kindles: Fenji's one brand identity, ember on ink with gold for people and the 焚 seal, for the launcher, the host console, the website and the docs."
colors:
  ink-950: "#0c0c0f"
  ink-900: "#111115"
  ink-850: "#16161b"
  ink-800: "#1c1c22"
  line: "rgba(255, 255, 255, 0.07)"
  line-strong: "rgba(255, 255, 255, 0.12)"
  outline: "rgba(255, 255, 255, 0.16)"
  field-line: "#706b63"
  scrim: "rgba(6, 6, 8, 0.6)"
  scrim-light: "rgba(6, 6, 8, 0.42)"
  text: "#f2efe9"
  text-prose: "#d8d2c8"
  text-2: "#c9c2b7"
  muted: "#a8a196"
  faint: "#8c867c"
  on-ember: "#ffffff"
  ember-600: "#c8431f"
  ember-hover: "#d24b25"
  ember-500: "#e4572e"
  ember-300: "#ff7a45"
  ember-glow: "#ffb070"
  ember-tint: "#2a1712"
  gold-300: "#f2b365"
  gold-100: "#ffd9ad"
  vignette-warm: "#2a1510"
  vignette-warm-mid: "#140d0c"
  vignette-cool: "#17171c"
  vignette-cool-mid: "#111114"
  ok: "#7fd38f"
  warn: "#e8b04a"
  danger: "#f26d7d"
  info: "#9cc3e4"
typography:
  display:
    fontFamily: "Cinzel, serif"
    fontSize: "64px"
    fontWeight: 600
    lineHeight: 1.02
    letterSpacing: "0.03em"
  headline:
    fontFamily: "Cinzel, serif"
    fontSize: "45px"
    fontWeight: 600
    lineHeight: 1.1
    letterSpacing: "0.03em"
  dialog-title:
    fontFamily: "Cinzel, serif"
    fontSize: "26px"
    fontWeight: 600
    lineHeight: 1.15
    letterSpacing: "0.02em"
  popover-title:
    fontFamily: "Cinzel, serif"
    fontSize: "20px"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0.03em"
  seal:
    fontFamily: "Cinzel, serif"
    fontSize: "18px"
    fontWeight: 600
    lineHeight: 1
  play:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "17px"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0.2em"
    fontFeature: "\"tnum\" 1"
  title:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 600
  status:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "15.5px"
    fontWeight: 400
    fontFeature: "\"tnum\" 1"
  body:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.6
  body-small:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "14.5px"
    fontWeight: 400
    lineHeight: 1.5
  button-primary:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "14.5px"
    fontWeight: 600
  button-secondary:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "14.5px"
    fontWeight: 500
  secondary:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "13.5px"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "13.5px"
    fontWeight: 600
  small:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "12.5px"
    fontWeight: 600
  mono:
    fontFamily: "Geist Mono, ui-monospace, monospace"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.75
    fontFeature: "\"tnum\" 1"
  mark:
    fontFamily: "Noto Serif SC, serif"
    fontWeight: 700
rounded:
  chip: "6px"
  control: "8px"
  card: "12px"
  pill: "999px"
  dot: "50%"
spacing:
  "4": "4px"
  "6": "6px"
  "8": "8px"
  "10": "10px"
  "12": "12px"
  "14": "14px"
  "16": "16px"
  "18": "18px"
  "20": "20px"
  "24": "24px"
  "28": "28px"
  "32": "32px"
  "40": "40px"
  "44": "44px"
  "56": "56px"
  "72": "72px"
components:
  rail:
    backgroundColor: "{colors.ink-900}"
    padding: "16px 0"
    width: "72px"
  brand-seal:
    backgroundColor: "{colors.ember-600}"
    textColor: "{colors.on-ember}"
    typography: "{typography.mark}"
    rounded: "{rounded.control}"
    size: "36px"
  seal:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.text-2}"
    typography: "{typography.seal}"
    rounded: "{rounded.control}"
    size: "44px"
  seal-active:
    backgroundColor: "{colors.ember-tint}"
    textColor: "{colors.text}"
    typography: "{typography.seal}"
    rounded: "{rounded.control}"
    size: "44px"
  seal-add:
    textColor: "{colors.muted}"
    rounded: "{rounded.control}"
    size: "44px"
  seal-bar:
    rounded: "{rounded.chip}"
    width: "16px"
    height: "3px"
  seal-bar-online:
    backgroundColor: "{colors.ok}"
  seal-bar-maintenance:
    backgroundColor: "{colors.warn}"
  seal-bar-offline:
    backgroundColor: "{colors.faint}"
  seal-bar-blocked:
    backgroundColor: "{colors.danger}"
  seal-tip:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.text}"
    typography: "{typography.secondary}"
    rounded: "{rounded.control}"
    padding: "9px 12px"
  seal-menu:
    backgroundColor: "{colors.ink-850}"
    rounded: "10px"
    padding: "6px"
    width: "184px"
  top-bar:
    padding: "0 16px 0 72px"
    height: "56px"
  address:
    textColor: "{colors.faint}"
    typography: "{typography.mono}"
  game-chip:
    textColor: "{colors.text-2}"
    typography: "{typography.secondary}"
    rounded: "{rounded.pill}"
    padding: "0 13px"
    height: "32px"
  game-chip-open:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.text}"
  game-chip-warn:
    textColor: "{colors.warn}"
  avatar:
    backgroundColor: "{colors.ember-tint}"
    textColor: "{colors.gold-300}"
    typography: "{typography.small}"
    rounded: "{rounded.dot}"
    size: "32px"
  icon-button:
    textColor: "{colors.muted}"
    rounded: "{rounded.control}"
    size: "32px"
  icon-button-open:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.text}"
  window-control:
    textColor: "{colors.faint}"
    rounded: "{rounded.control}"
    width: "28px"
    height: "32px"
  button-play:
    backgroundColor: "{colors.ember-600}"
    textColor: "{colors.on-ember}"
    typography: "{typography.play}"
    rounded: "{rounded.control}"
    width: "248px"
    height: "64px"
  button-play-kindle:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.text}"
    typography: "{typography.play}"
    rounded: "{rounded.control}"
    width: "248px"
    height: "64px"
  button-play-closed:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.faint}"
    typography: "{typography.play}"
    rounded: "{rounded.control}"
    width: "248px"
    height: "64px"
  button-play-running:
    backgroundColor: "rgba(42, 23, 18, 0.6)"
    textColor: "{colors.text}"
    typography: "{typography.play}"
    rounded: "{rounded.control}"
    width: "248px"
    height: "64px"
  button-primary:
    backgroundColor: "{colors.ember-600}"
    textColor: "{colors.on-ember}"
    typography: "{typography.button-primary}"
    rounded: "{rounded.control}"
    padding: "0 18px"
    height: "44px"
  button-secondary:
    textColor: "{colors.text}"
    typography: "{typography.button-secondary}"
    rounded: "{rounded.control}"
    padding: "0 18px"
    height: "44px"
  button-small:
    typography: "{typography.secondary}"
    rounded: "{rounded.control}"
    padding: "0 14px"
    height: "36px"
  button-quiet:
    textColor: "{colors.muted}"
    typography: "{typography.body-small}"
  link:
    textColor: "{colors.gold-300}"
    typography: "{typography.button-secondary}"
  link-hover:
    textColor: "{colors.gold-100}"
  input:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.text}"
    typography: "{typography.body-small}"
    rounded: "{rounded.control}"
    padding: "0 14px"
    height: "46px"
  checkbox:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.gold-300}"
    rounded: "{rounded.chip}"
    size: "20px"
  switch-off:
    backgroundColor: "{colors.ink-800}"
    rounded: "{rounded.pill}"
    width: "40px"
    height: "24px"
  switch-on:
    backgroundColor: "{colors.gold-300}"
    rounded: "{rounded.pill}"
    width: "40px"
    height: "24px"
  status-dot:
    rounded: "{rounded.dot}"
    size: "8px"
  face:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.gold-300}"
    typography: "{typography.small}"
    rounded: "{rounded.dot}"
    size: "28px"
  progress-track:
    backgroundColor: "{colors.ink-800}"
    rounded: "{rounded.chip}"
    height: "4px"
  progress-fill:
    backgroundColor: "{colors.ember-500}"
    height: "4px"
  popover:
    backgroundColor: "{colors.ink-850}"
    rounded: "{rounded.card}"
    width: "404px"
  popover-head:
    typography: "{typography.popover-title}"
    padding: "0 12px 0 24px"
    height: "60px"
  dialog:
    backgroundColor: "{colors.ink-850}"
    rounded: "{rounded.card}"
    padding: "30px 32px 28px"
    width: "500px"
  command-line:
    backgroundColor: "{colors.ink-950}"
    textColor: "{colors.text}"
    typography: "{typography.mono}"
    rounded: "{rounded.control}"
    padding: "12px 14px"
  toast:
    backgroundColor: "{colors.ink-850}"
    rounded: "{rounded.card}"
    padding: "16px 12px 16px 18px"
    width: "360px"
  console-titlebar:
    backgroundColor: "{colors.ink-900}"
    textColor: "{colors.text-2}"
    typography: "{typography.mono}"
    height: "32px"
    padding: "0 24px"
  console-status:
    backgroundColor: "{colors.ink-850}"
    textColor: "{colors.text-2}"
    typography: "{typography.mono}"
    height: "40px"
    padding: "0 24px"
  console-log:
    backgroundColor: "{colors.ink-950}"
    textColor: "{colors.text-2}"
    typography: "{typography.mono}"
    padding: "14px 24px 12px"
  console-palette:
    backgroundColor: "{colors.ink-850}"
    rounded: "0"
    padding: "6px"
    width: "680px"
  console-prompt:
    backgroundColor: "{colors.ink-900}"
    textColor: "{colors.text}"
    height: "48px"
    padding: "0 24px"
---

# Design System: Fenji

## Overview

**Creative North Star: "One stage, PLAY kindles"**

Fenji is named for 焚寂, the cursed sword, and its light now lives in the one button the player came to press. The chosen server fills the window: its name inscribed in Cinzel, one line of state, its host's notice, PLAY, and the friends already in the lobby. While the game's files verify or Proton prepares, ember fills PLAY from the left; when the copy is ready, PLAY is alight and the stage's ground warms from the lower left. Readiness is felt on PLAY before it is read.

Everything else is quiet ink: a near-black stage, an ink-900 rail of square server seals, hairlines, warm off-white text. Ember is rationed to one action in view, gold marks people and places to go, and the four status hues only ever travel with words. Game, Account and Settings open as compact popovers under their own controls and close again; a task the player must finish takes the stage or a centred dialog; an update waits in a corner. The finish is a modern game launcher's (HoYoPlay, Riot Client, Battle.net) in xianxia restraint: no tabs, no side column, no key art, no news. The Ember Sword's launcher parts (the blade, the header with its wordmark and tabs, the context column) are retired; their boards stay in `docs/design/launcher/boards/` as the v0.5 record.

These tokens are Fenji's single identity, approved by the owner on 2026-10-13 and kept through the launcher's redesign of 2026-10-16, for the launcher, the host console, the website and the docs. The fonts (Cinzel, Geist, Geist Mono, and Noto Serif SC subset to 焚寂) are SIL Open Font License and ship inside each product; the launcher's content security policy is `default-src 'self'`, so nothing loads from a font CDN. The redesign's boards (`docs/design/launcher/redesign/screens.html`) are what the build follows, under the rulings in its spec (`docs/superpowers/specs/2026-10-16-launcher-redesign-design.md`); where the build departs from a board, this file records the build and says why.

**Key Characteristics:**
- Ink ground in tonal steps with hairlines; a shadow only on what floats over the stage.
- One ember action in view, at most; gold for people and links; status colours only beside words.
- PLAY kindles: it is the progress indicator and the brand's light at once.
- A 72 px rail of square ink seals, each over a 3 px state bar.
- Cinzel for the server's name and titles, Geist for the interface, Geist Mono for addresses, paths and the console.
- A fixed 1200 × 720 window: the rail, a 56 px top bar, and the stage.
- Short, true words; never a request for Steam or Gameforge credentials.

## Colors

A warm-neutral ink palette with a single ember voice, a gold accent for people and links, and four quiet status hues.

### Primary
- **Forge Ember** (ember-600): lit PLAY's fill, under a 10% white sheen; the fill of the one primary action of a task, a popover section or a dialog (Continue, Create account, Sign in, Add, Use it, Repair, Verify, Remove); and the 焚 seal. Always with white text (4.9:1).
- **Hover Ember** (ember-hover): lit PLAY and an ember button under the pointer, a step brighter than ember-600. Never at rest.
- **Live Ember** (ember-500): where the player is and how far along: the active seal's 1 px edge, + while Add a server is open, the account's ring while its popover is open, the 2 px focus ring, the caret, text selection, the Game popover's 4 px progress fill, the bright end of PLAY's kindle fill (at 85%), the running PLAY's edge (at 55%) and lit PLAY's glow (at 70%).
- **Ember Edge** (ember-300): the 1 px inner edge of lit PLAY (at 50%, 70% on hover) and of an ember button (at 45%, 65% on hover). Never a fill.
- **Kindle Glow** (ember-glow): the soft glow beside the kindle's leading edge (at 55%). Never on chrome.

### Secondary
- **Lantern Gold** (gold-300): people and places to go: the host's name before their notice, friends' names and initials, the account's initials, "Signed in as lin_yue", links, and the console's player names and prompt; and a setting that's on (the switch's track, the checkbox's tick).
- **Kindle Tip** (gold-100): the kindle's 2 px leading edge, and a link on hover.

### Tertiary (status)
- **Jade OK** (ok): online, Game ready, Verified, the running PLAY's dot; a seal's bar, an 8 px dot or a check.
- **Amber Warn** (warn): maintenance (a seal's bar, the state line's dot); a file that needs repair or a missing folder (the Game chip's words, dot and 40% ring, a repair's reasons); console WARN.
- **Cinnabar Danger** (danger): a changed key, a key not yet trusted or a server this launcher can't play on (a seal's bar); errors and an unreachable server, always as the circled exclamation and words; an invalid field's ring and its message; console ERROR.
- **Mist Info** (info): the info mark before the model's notice.

### Neutral
- **Ink 950** (ink-950): the stage's ground, the command line's well, the switch's knob when on, the ring between overlapping faces, and the console log.
- **Ink 900** (ink-900): the rail, and the console's title bar and prompt row.
- **Ink 850** (ink-850): what floats over the stage: popovers, dialogs, the toast, a seal's menu; the console's status strip and command palette.
- **Ink 800** (ink-800): seals, PLAY while it kindles or is closed, fields, checkboxes, the switch's track when off, faces, a seal's tip, a control while its popover is open, a menu row on hover, the progress track, the scrollbar's thumb; the console's chosen palette row.
- **Line** (line, white 7%): hairlines: the rail's edge, a popover's head and sections, file rows, the window controls' divider, the command line's ring, and closed PLAY's edge.
- **Line Strong** (line-strong, white 12%): the ring round everything that floats (tip, menu, popover, dialog, toast), the Game chip's ring, and kindling PLAY's edge.
- **Outline** (outline, white 16%): an outline button's ring, the ring of + and of a seal under the pointer, a checkbox's ring, the Game chip's ring on hover or while open.
- **Field Line** (field-line): a field's ring and the switch's track when off: 3.2:1 against the ink-800 fill and 3.4:1 or more against the grounds around it, so a field's edge meets the 3:1 non-text floor.
- **Scrim** (scrim, ink at 60%): behind a dialog. Darker than the boards' 42% under Add a server on purpose: 42% over a lit PLAY would leave two embers in view.
- **Scrim Light** (scrim-light, ink at 42%): behind a popover; the top bar stays above it.
- **Paper** (text): the server's name, titles and primary text (17:1 on ink-950).
- **Prose** (text-prose): running sentences: the host's notice, a task's lede, a dialog's description, the stage's notes and What to try (13:1).
- **Parchment** (text-2): the state line, field labels, a seal's letter, the Game chip's words, a job's words in the Game popover (11:1).
- **Ash** (muted): meta lines, a popover's lines, "playing", hints, quiet words, icon buttons, the + seal, placeholders (7.6:1).
- **Smoke** (faint): the address, closed PLAY's word (4.7:1 on ink-800), the window controls, footnotes, an offline seal's bar, What to try's bullets; console timestamps and separators (5.4:1 on ink-950).
- **Ember Tint** (ember-tint): the active seal's ground, the account's ground, the player's own face, and the running PLAY's ground (at 60%).
- **On Ember** (on-ember): white on ember: PLAY's word, an ember button's, the 焚, and selected text.
- **Warm Ground** (vignette-warm, then vignette-warm-mid): the stage's ground, from the lower left, while PLAY is lit or the game runs.
- **Cool Ground** (vignette-cool, then vignette-cool-mid): the stage's ground while PLAY kindles, while the copy needs something, and behind a task.

### Named Rules
**The One Ember Rule.** One ember-600 action in view, at most: PLAY when it's lit, otherwise the one primary action of a task, a popover section or a dialog. Ember-500 marks where the player is (the active seal, focus, the caret, a selection, an open account) and progress. Nothing else is ember: not icons, not headings, not links, not switches.

**The Gold Is People Rule.** Gold marks people and places to go, and a setting that's on. It never fills an action button.

**The Status Never Alone Rule.** A status colour always travels with words, and an error also carries the circled exclamation. In the rail, a seal's bar has its words in the seal's tip and its accessible name.

## Typography

**Display Font:** Cinzel (with serif)
**Body Font:** Geist (with system-ui, sans-serif)
**Label/Mono Font:** Geist Mono (with ui-monospace, monospace), tabular numbers
**Mark:** Noto Serif SC 700, subset to the two glyphs 焚寂

**Character:** Cinzel's inscribed capitals give a server's name the weight of a carved plaque; Geist keeps the interface crisp and modern beneath it; Geist Mono makes addresses, paths and the console line up.

The titles sit on the Minor Third (1.200) from a 15 px base, rounded to whole pixels: 26 for dialogs, 45 for tasks and 64 for the server's name (steps 3, 6 and 8). The interface uses named sizes between the steps: 20 for popover titles, 18 for a seal's letter, 17 for PLAY, 16 for a popover section's heading, 15.5 for the state line and a task's lede, 14.5 for controls and the stage's notes, 13.5 for secondary text and labels, 13 for Geist Mono, and 12.5 for initials, the floor.

### Hierarchy
- **Display** (Cinzel 600, 64 px, line-height 1.02, tracked 0.03em): the server's name on the stage, up to two lines, then clipped. Cinzel draws it in capitals.
- **Headline** (Cinzel 600, 45 px, line-height 1.1, tracked 0.03em): a task's title: "Welcome to Fenji", "Join Moonward", "Sign in to Moonward".
- **Dialog title** (Cinzel 600, 26 px, line-height 1.15, tracked 0.02em): "Add a server", "Remove Moonward?", "A new key for Moonward", "Run it yourself".
- **Popover title** (Cinzel 600, 20 px, line-height 1, tracked 0.03em): Game, Account, Settings.
- **Seal** (Cinzel 600, 18 px): a server's initial in its seal.
- **Play** (Geist 600, 17 px, tracked 0.2em, in capitals, tabular numbers): PLAY's word: "Play", "Verifying 78%", "Starting", "Running".
- **Title** (Geist 600, 16 px): a popover section's heading: "Swords of Legends Online", "Steam library", "Moonward", "Auto-update".
- **Status** (Geist 400, 15.5 px, tabular numbers): the state line in text-2; a task's lede in Prose at line-height 1.5.
- **Body** (Geist 400, 15 px, line-height 1.6): sentences; the host's notice and a dialog's description at 1.55, in Prose, up to 560 px wide.
- **Body small** (Geist 400, 14.5 px, line-height 1.5): the stage's notes and What to try in Prose, a field's text, a checkbox's label, quiet words.
- **Button primary** (Geist 600, 14.5 px) and **Button secondary** (Geist 500, 14.5 px): ember buttons; outlines and gold links. Sentence case; 13.5 px in the small size and in a popover's links.
- **Secondary** (Geist 400, 13.5 px, line-height 1.5): the Game chip, a seal's tip, meta lines, a popover's lines, hints, field errors, the friends' row, the toast's line.
- **Label** (Geist 600, 13.5 px): field labels, in text-2, 10 px above the field.
- **Small** (Geist 600, 12.5 px): initials on the account and the faces. The floor: no readable text is smaller.
- **Mono** (Geist Mono 400, 13 px, tabular numbers): the address in the top bar, paths, a repair's file names, the command in Run it yourself; mono fields at 13.5 px; and the whole host console (line-height 1.75; 14 px on the prompt). A path under the player's home folder shows it as `~` ("~/Games/SoLO"), for display only, so a screenshot carries no player's name; on Windows, where `~` means nothing to a player, paths show whole.
- **Mark** (Noto Serif SC 700): 焚 in the rail's seal, at 20 px.

### Named Rules
**The Plaque Rule.** Cinzel is only for the server's name, the titles of tasks, dialogs and popovers, and a seal's letter. PLAY, buttons, labels and lines are Geist; addresses, paths and commands are Geist Mono.

**The Tracked Caps Rule.** PLAY is the only tracked uppercase label (0.2em). Every other button is sentence case and untracked.

## Layout

The launcher is a fixed 1200 × 720 window without the system's frame: no resizing and no maximise, minimise and close only, in its own top bar. The empty space of the rail and of the top bar drags the window.

The window is two columns: the rail (72 px) and the stage (1128 px). The stage holds the top bar (56 px), the stage's body, and the layers that float over it: popovers, dialogs and the toast. Scrims cover the stage's body: the top bar stays above them, and the rail lit beside them.

- **The rail:** the 焚 seal 16 px from the top, then the seals, 44 px and 14 px apart (their state bars sit in the gap), then +. With more servers than fit, past nine or so, the seals scroll with the scrollbar hidden, keeping 4 px above and 8 px below so an edge or a bar is never clipped, and + stays below them, in reach. With no servers (Welcome), + sits straight under the seal.
- **The top bar:** 72 px in from the stage's left and 16 px from its right, controls 14 px apart, so the address starts at x 144, in line with the stage's group.
- **The stage's group:** left-aligned at x 144 (72 px into the stage) and centred vertically in the window, between the 56 px top bar and a 56 px inset at the bottom, at most 820 px wide. Its beats: the name; the state line 14 px below; the host's notice 18 px below; the row 44 px below (PLAY, then its one action 18 px after it); a note, What to try or a meta line 14 to 18 px below; who's playing 40 px below the last of them. The group grows with its lines and stays centred.
- **A task** (Welcome, Join, Sign in) takes the group's place, at most 560 px wide: the title, the lede 14 px below it and 28 px above the form, fields 16 px apart, the actions 24 px below, 18 px apart.
- **Popovers** are 404 px wide, 60 px from the top and 16 px from the stage's right edge, under their controls, sized to their content and at most the stage's height less 76 px; their bodies scroll. **Dialogs** centre on the stage at 500 px. **The toast** sits 24 px from the stage's bottom right, at 360 px.

Spacing runs on 4, 6, 8, 10, 12, 14, 16, 18, 20, 24, 28, 32, 40, 44, 56 and 72: 4 to 10 px inside a control (an icon's gap, a label's space), 12 to 18 px between controls and lines, 20 to 28 px for a section's padding and the space before actions, 40 to 56 px for the stage's beats, and 72 px for the stage's inset. A few values sit between the steps (9 px for a mark's gap and a tip's padding, 13 px for the Game chip's sides, 30 px above a dialog's title); they stay where they are and don't spread.

The host console fills its terminal: a 32 px title bar, a 40 px status strip, the log filling the middle and anchored to the bottom, an optional command palette, and a 48 px prompt row. 24 px side padding throughout.

## Elevation & Depth

Flat by default, layered by tone. Depth comes from the ink ramp (950 the stage, 900 the rail, 850 what floats, 800 controls and fields) and from 1 px hairlines drawn as inset rings, which never move the layout. A shadow means the thing floats over the stage, and it always comes with a 1 px line-strong ring; shadows are black and soft, never coloured or offset hard. The one light in the system is PLAY's.

### Shadow Vocabulary
- **Tip** (`box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.12), 0 12px 30px rgba(0, 0, 0, 0.45)`): a seal's tip.
- **Menu** (`box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.12), 0 16px 40px rgba(0, 0, 0, 0.5)`): a seal's menu.
- **Toast** (`box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.12), 0 20px 50px rgba(0, 0, 0, 0.55)`): the update toast.
- **Popover** (`box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.12), 0 28px 70px rgba(0, 0, 0, 0.6)`): Game, Account and Settings, over the light scrim.
- **Dialog** (`box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.12), 0 30px 80px rgba(0, 0, 0, 0.6)`): a dialog, over the scrim.
- **PLAY's light** (`box-shadow: inset 0 0 0 1px rgba(255, 122, 69, 0.5), 0 14px 40px -12px rgba(228, 87, 46, 0.7)`; on hover `inset 0 0 0 1px rgba(255, 122, 69, 0.7), 0 18px 44px -12px rgba(228, 87, 46, 0.8)`): lit PLAY's ember edge and the glow beneath it.
- **The kindle's edge** (`box-shadow: 6px 0 14px -2px rgba(255, 176, 112, 0.55)` beside a 2 px gold-100 edge): the leading edge of PLAY's fill while it kindles. Gone when motion is reduced.
- **Rings** (`box-shadow: 0 0 0 2px #0c0c0f` between overlapping faces; `0 0 0 2px #e4572e` round the account while its popover is open): separation and an open state, not depth.

### Named Rules
**The Hairline Rule.** Surfaces separate by tone and a 1 px hairline, never by shadow. A shadow belongs only to what floats over the stage, with its line-strong ring.

**The One Light Rule.** PLAY's ember is the only light: the glow under it when lit, the kindle's gold edge, and the stage's ground, which warms with it. Nothing else glows.

## Shapes

Gently rounded and consistent: 8 px for seals, PLAY, buttons, fields, icon buttons, a seal's tip and the command line; 12 px for popovers, dialogs and the toast; 6 px for checkboxes, a menu's rows, the seals' state bars and the progress bar; 10 px for a seal's menu; a full pill (999 px) for the Game chip and the switch; circles for the account, the faces, status dots and the switch's knob. Servers are squares, not circles: the rail's own vocabulary, not a chat app's round avatars with presence dots. The host console's command palette is square (0 radius), as a terminal is.

Lines are 1 px, drawn as inset rings: the rail's right edge, a popover's head and sections, a file row, the window controls' divider, and every control's ring. The focus ring and the kindle's gold edge are 2 px, as are the rings round an open account and between faces. The state bar under a seal is 3 px.

Icons are 1.8 px outline strokes (2 px for the circled exclamation) on a 24 px grid with round caps and joins, drawn at 12 to 20 px, coloured by `currentColor` or a status hue; the window controls and the check use a 12 px grid. Settings is an eight-toothed cog round its hub: a hub with rays reads as light and dark mode.

The 焚 seal is a square ember tile with gently rounded corners and the glyph in white: 36 px with 8 px corners and a 20 px glyph in the rail. The app icon is the same seal at the proportions of a 26 px tile, with 6 px corners and the glyph at 14 px, scaled to 1024 px (`apps/launcher/src-tauri/icons/`).

## Components

### The rail
- **Structure:** 72 px of ink-900 with a 1 px line on its right edge and 16 px of padding at top and bottom. From the top: the 焚 seal (36 px, ember-600, 8 px corners, white 焚 in Noto Serif SC 700 at 20 px), the seals, 14 px apart, then +, a 44 px outline seal with a 20 px plus in muted. Its tips and menus float over the stage, and no scrim covers it.
- **A seal:** a 44 px square of ink-800 with 8 px corners and the server's initial in Cinzel 600 18 px, text-2. Under the pointer it takes text colour and a 1 px outline ring. The active seal has an ember-tint ground, text colour and a 1 px ember-500 edge, and is `aria-current`. + takes the same ember edge under the pointer and while Add a server is open.
- **State bar:** 3 px tall and 16 px wide (14 px in from each side), 5 px under its seal, with 6 px ends: jade online, amber in maintenance, faint offline or not yet known, cinnabar for a changed key, a key not yet trusted, or a server this launcher can't play on.
- **Tip:** on hover or focus, beside the rail at x 66: ink-800, 8 px corners, 9 by 12 px of padding, the tip shadow, 13.5 px: the server's name in 600, then its players or its state in muted, and its region when its host set one ("Moonward 3/10 players · Germany").
- **Menu:** a right click, the context-menu key or Shift+F10 opens a seal's menu beside it: ink-850, 10 px corners, 6 px of padding, at least 184 px wide, the menu shadow; 36 px rows of 14 px text with 6 px corners, ink-800 under the pointer: "Source code" (when the server publishes it) and "Remove from list". Delete asks to remove the server too. A probe never closes an open menu.
- **Keyboard:** the arrow keys, Home and End move along the seals and +.

### The top bar
- **Structure:** 56 px, above the stage's body and above a popover's or a dialog's scrim: the address at the left, then the Fenji the server runs as faint after it (`play.example.net · Fenji 0.5.3`, from its last answer; nothing when it never said; the owner's request, #408, after the boards), the version never cut short where a long address takes an ellipsis, and the controls never shrink for them; then the Game chip, the account, Settings, a 1 px line, and Minimise and Close, 14 px apart. Under a dialog, Minimise, Close and the drag still work, and the chip, the account and Settings wait (inert) until it closes.
- **Address:** the active server's address in Geist Mono 13 px, faint, cut with an ellipsis at 420 px.
- **Game chip:** a 32 px pill with 13 px sides and a 1 px line-strong ring: an 8 px dot and two or three words in 13.5 px text-2. "Game ready" has a jade dot; "Verifying", "Downloading 42%", "Set up the game" and "Not verified" a muted one; "1 file needs repair" and "Game folder missing" are amber words with an amber dot and a 40% amber ring. Under the pointer it takes text colour and an outline ring; open, ink-800 with an outline ring.
- **Account:** a 32 px circle of ember-tint with the player's initials in gold 12.5 px 600 and a 25% gold ring (55% under the pointer); a person figure while signed out. Open, a 2 px ember-500 ring.
- **Settings and the window controls:** 32 px icon buttons in muted, text colour under the pointer; Settings shows an ink-800 tile while open. After the 1 px line, Minimise and Close are 28 by 32 px in faint. There is no maximise.
- **Per screen:** Welcome shows Settings and the window controls only; Join adds the address; Sign in adds the Game chip; the stage shows them all.

### The stage
The chosen server fills the stage: its name, one line of state, the host's notice, PLAY with at most one action beside it, a line only when one is needed, and who's playing. It updates in place, so PLAY and the action beside it keep a player's focus while progress streams in.
- **Grounds:** two radial grounds from the lower left (1000 by 620 px at 6% 104%) cross-fade over 1200 ms. Warm (vignette-warm, then vignette-warm-mid at 48%, clear by 78%) while PLAY is lit or the game runs. Cool (vignette-cool, then vignette-cool-mid) while PLAY kindles, while the copy needs setting up, checking or repair, and behind a task. Plain ink-950 when Play can't happen: maintenance, offline, a key or version problem, an unsupported system.
- **State line:** Status type in text-2: a mark, then plain words joined by " · ". The mark is an 8 px dot (jade online, amber in maintenance, muted while connecting) or, for an error, the 18 px circled exclamation in cinnabar: "Online · 3/10 players · 24 ms · Germany", "Maintenance · back around Thursday 21:00", "Offline · last seen yesterday 18:42", "Key changed", "Not trusted yet", "Runs a newer Fenji", "Expects another game build", "Connecting…". The ping shows only while PLAY is lit and sits beside the line's live region, hidden from assistive technology, so a screen reader hears the state when it changes, not every probe's ping. The server's region, when its host set one, closes the line online, in maintenance and offline, read with it but outside the live region; one too long for the line ends in an ellipsis, and the facts before it never give way.
- **The host's notice:** Body in Prose, up to 560 px, led by the host's name and a colon in gold: "Wen: the lobby is open." In maintenance, the host's maintenance message; for a key or version problem, one sentence on what to do instead ("This isn't the server you trusted. Ask Wen for a new link."); offline or on an unsupported system, nothing.
- **Under the row, only when needed:** What to try (offline: three steps in 14.5 px Prose with faint 5 px bullets, opened by its gold link); the model's notice (14.5 px Prose after an info mark, or the circled exclamation for an error, with Dismiss); a meta line in 13.5 px muted ("Setting up Proton (first start only)", "Since 20:05", "Playing on Windows isn't supported yet.").
- **Who's playing:** 40 px under the row, up to four 28 px faces (ink-800, gold initials, a 2 px ink-950 ring, overlapping by 6 px) and "Wen, Shu, Kai playing", the names in gold and "playing" in 13.5 px muted; past three names, "and 2 more playing". While the game runs the player leads, as "You", on ember tint. It shows only where a player could be in the lobby.

**The Ground Follows PLAY Rule.** The ground is warm only while PLAY is lit or the game runs, cool while something is being readied or a task holds the stage, and plain ink when Play can't happen. It shows nothing else.

### PLAY (signature component)
PLAY kindles. It's 248 by 64 px with 8 px corners, its word in Geist 600 17 px, tracked 0.2em, in capitals, with tabular numbers and a soft shadow under the letters (`0 1px 2px` black at 40%). It has four looks, and it changes slowly (its fill and edge over 700 ms, its word over 400 ms): the button the player came to press warms.
- **Lit:** ember-600 under a 10% white sheen on its top half, "Play" in white, and PLAY's light; ember-hover under the pointer, its light stronger. Pressed, it sinks 1 px. The only look that can be pressed.
- **Kindling:** ink-800 with a line-strong edge, its word in text colour ("Verifying 78%", "Starting"), filled from the left to the progress with ember (ember-600 at 45% to ember-500 at 85%); the fill's leading edge is a 2 px gold-100 line with the kindle glow beside it, and it follows the progress over 600 ms. A hidden progress bar names the task and its percentage for assistive technology.
- **The drift:** with no progress to show, as on most starts, the fill breathes from the left edge between 12% and 60% of the width, 2.6 s each way; it rests at 36% when motion is reduced.
- **Kindled:** when a check ends and PLAY lights, the fill completes and dissolves over 600 ms while the ember climbs beneath it, so the ember never drops out at the moment it exists for. A start that ends lit (cancelled or failed) skips it, as does reduced motion.
- **Closed:** ink-800 with a line edge and "Play" in faint, the not-allowed cursor: Play can't happen, and the state line or the meta line says why.
- **Running:** ember-tint at 60% with a 1 px ember-500 edge at 55%, a jade dot and "Running" in text colour; "Since 20:05" below.

**The Kindle Rule.** PLAY's fill is the stage's only ember motion, and it shows only what stands between the player and Play: a check of their files, or the game starting. A download's progress shows on the Game chip and in the Game popover's bar.

### One line of state, one action
Every state of the stage has one line of state and at most one action beside PLAY, and that action is the state's cure. It's a small outline 18 px after PLAY, or quiet words for Stop.

| State | PLAY | Beside it | Ground |
|---|---|---|---|
| Ready | lit | nothing | warm |
| Verifying | kindling, "Verifying 78%" | Stop (quiet) | cool |
| Starting | kindling, "Starting", drifting | Cancel | cool |
| Running | running | nothing; "Since 20:05" below | warm |
| Maintenance | closed | Refresh | plain |
| Offline | closed | Retry, and the gold What to try | plain |
| A changed or untrusted key | closed | Paste new link or Paste link (opens Add a server) | plain |
| Another version | closed | Check for updates | plain |
| The copy isn't ready | closed | Set up the game, Repair, Find the game or Verify (opens the Game popover); nothing while a download runs, since the Game chip carries its progress | cool |
| An unsupported system | closed | nothing; "Playing on Windows isn't supported yet." below | plain |

**The One Line, One Action Rule.** One line says the state; one action, beside PLAY, cures it. No second button, no paragraph: where more is needed, a link opens it (What to try) or a popover holds it (Game).

### Buttons
- **Ember (primary):** 44 px tall with 18 px sides, ember-600, white Geist 600 14.5 px, 8 px corners, a 1 px ember-300 edge at 45%; under the pointer, ember-hover with the edge at 65%. The one primary action of a task, a popover section or a dialog: Continue, Create account, Sign in, Add, Remove, Use it, Repair, Verify, Find the game, Use the new key. In popovers it's the small size.
- **Outline (secondary):** 44 px, no fill, a 1 px outline ring, text-colour Geist 500 14.5 px; under the pointer a 4% white fill and a 28% white ring. Small (36 px, 14 px sides, 13.5 px) beside PLAY, in popovers and in the toast: Refresh, Retry, Cancel, Verify integrity, Sign out, Restart, Copy.
- **Link:** gold-300 Geist 500 14.5 px (13.5 px in popovers), without an underline until the pointer is on it, when it turns gold-100 and underlines 3 px below. A place to go or a thing to open: "I have an account", "What to try", "Change folder", "Run it yourself", "Sign in", "Check now", "Logs".
- **Quiet:** muted words, text colour under the pointer: "Stop" beside a check, "Cancel" on Sign in.
- **Icon buttons:** 32 px, 8 px corners, muted, text colour under the pointer, always named for assistive technology: Settings, Close, Dismiss, Show password, Later.
- **Disabled:** a button waiting on its send drops to 55% opacity. PLAY is never dimmed; it closes.

### Inputs / Fields
- **Style:** 46 px, ink-800, a 1 px field-line ring, 8 px corners, 14 px sides, Geist 14.5 px in text colour; a slightly lighter ring under the pointer. Addresses, invite links and paths are Geist Mono 13.5 px.
- **Label:** above the field, 13.5 px 600 in text-2, 10 px gap. Welcome names its one field only for assistive technology, since its lede says what to paste.
- **Hint:** 13.5 px muted, 8 px under the field: "Not your Steam password."
- **Focus:** the 2 px ember-500 ring, 2 px off the field.
- **Error:** a 1 px cinnabar ring, `aria-invalid`, and under the field the 14 px circled exclamation and a 13.5 px line in cinnabar saying what happened and what to try: "Paste the invite link first.", "Usernames are 3 to 16 letters, digits, _ or -." A refusal that concerns no one field shows the same way under the form.
- **Show:** a password field carries Show, an eye icon button inside its right edge, so one field can catch a typo.
- **Checkbox:** 20 px, 6 px corners, ink-800 with an outline ring and a gold tick; its 14.5 px text-2 label is clickable: "Remember this server".
- **Switch:** a 40 by 24 px pill. Off, ink-800 with a field-line ring and a muted 18 px knob; on, gold-300 with an ink-950 knob. A real checkbox with `role="switch"` underneath.
- **A field that's a button:** Settings' Download folder looks like a mono field and opens the folder picker.

### Popovers
Game, Account and Settings open under their own control in the top bar, and close again.
- **Shape:** 404 px wide, ink-850, 12 px corners, the popover shadow; it pops in from its top right corner over 220 ms. The light scrim fades in over the stage, and the top bar stays above it with the popover's control shown open.
- **Head:** 60 px: the title in Cinzel 600 20 px, 24 px in, Close at the right, a hairline below.
- **Body:** 24 px sides. Sections of 20 px above and below, hairlines between, none after the last: a 16 px 600 heading, 13.5 px muted lines 6 px below it (a jade dot before "Verified"), paths in Geist Mono 13 px, then the actions 16 px below, 12 px apart (links alone, 20 px apart). A refusal shows under the sections, as a field's error does. The body scrolls when it's taller than the stage.
- **Keyboard:** focus moves in as it opens, stays inside while it's open, and goes back to its control when it closes. Escape, the scrim or Close closes it.
- **Game:** the player's copy as it is. Ready: "Swords of Legends Online", its path, "Verified" and when, then Verify integrity (an outline) and Change folder. Not verified: Verify in ember. Set up: Steam library (its path, and Use it in ember when it's the build the server expects), Another folder (Choose folder, in ember when there's no Steam copy to use), and Download ("With your Steam account", an outline, then Run it yourself). Working: the job's word and percentage over the 4 px bar, then Cancel or Stop; while DepotDownloader waits for a Steam sign-in, "Sign in to Steam in its own window. Fenji never sees it." and Run it yourself. Needs repair: "1 file needs repair", up to six file rows (the path in mono, the reason in amber: Missing, Wrong size, Changed), Repair in ember with Change folder and Run it yourself, and "Put them back another way? Verify". Folder missing: its path and Find the game in ember.
- **Account:** a section per server: its name; "Signed in as lin_yue", the name in gold, with Sign out on the active server; or "Not signed in · Sign in", or "Invited · Create account", as gold links. A footnote in faint: "To delete an account, ask the host."
- **Settings:** Auto-update with its switch, over "0.5.1 · up to date · Check now" (Restart once an update is ready); on Linux, umu-run (a mono field, "Automatic") and Download folder; then Logs, Licences, Source and Privacy as gold links.

### Dialogs
- **Shape:** 500 px, centred on the stage, ink-850, 12 px corners, 30 px of padding above, 32 px at the sides and 28 px below, the dialog shadow. It pops in over 220 ms over the scrim, which covers the stage's body but not the top bar or the rail. `role="dialog"` with `aria-modal`; focus stays inside and goes back where it was when it closes, or to the dialog it closes onto; Escape or the scrim closes it.
- **Anatomy:** the title in Cinzel 600 26 px; a description 10 px below in 15 px Prose (line-height 1.55); the body 18 px below; then the actions at the bottom right, 24 px below and 12 px apart: the outline first, then the one ember.
- **Add a server** (from +, or from Paste new link): "Address or invite link" in a mono field, then Cancel and Add; a refusal shows under the field.
- **A key** ("A new key for Moonward"): what the link carries and whom to trust, then Keep the old key and Use the new key. Escape keeps the old key.
- **Remove** ("Remove Moonward?"): "Your account on it stays. To add it again, you'll need its link or address.", then Cancel and Remove.
- **Run it yourself:** the DepotDownloader command in a command line (ink-950, a line ring, 8 px corners, 12 by 14 px of padding, Geist Mono 13 px, selected whole with one click) with a small Copy; where to get the tool first, as a gold link; then Close and Choose folder or Verify.

### The update toast
360 px, 24 px from the stage's bottom right, ink-850, 12 px corners, 16 px of padding above and below, 18 px on the left and 12 px on the right, the toast shadow; it rises 10 px as it fades in, over 360 ms. "Update ready" (14.5 px 600) over "0.5.1 · installs on restart" (13.5 px muted), a small Restart outline, and Later as a 28 px icon button in faint. `role="status"`. It never takes the stage and is never ember: PLAY stays the one ember. Settings keeps Restart after Later.

### Task screens
The stage holds the one task in the group's place, over the cool ground: Welcome (no server yet), Join (an account on the server that invited the player) and Sign in.
- **Structure:** up to 560 px: the title in Cinzel 45; a lede in 15.5 px Prose (line-height 1.5); the fields, 16 px apart; then the actions: the ember action, and a gold link or quiet words.
- **Welcome:** "Welcome to Fenji", "Paste the invite link your host sent you.", one mono field, Continue, and "I have an account", which asks for the server's address instead and goes on to sign in.
- **Join:** "Join Moonward", "Hosted by Wen." with the name in gold, Username, Password with Show and "Not your Steam password.", then Create account and "I have an account".
- **Sign in:** "Sign in to Moonward", "Your Moonward account, not Steam.", Username, Password with Show, "Remember this server" (ticked), then Sign in and Cancel (quiet).

### Words
- **The glossary:** Verify integrity (the full check), Verify (the quick check), Verifying, Verified, Repair, Game, Online, Maintenance, Offline, Key changed, Sign in and Sign out, Create account, Add.
- **Short and true:** a state is a few words joined by " · ", in the player's local time: "back around 21:00", "last seen yesterday 23:10", "Since 20:05". Sentence case everywhere but PLAY. An error says what happened and what to try.
- **No build numbers and no file counts:** progress is a percentage ("Verifying 78%", "Downloading 42%"). The only such numbers on screen are how many files need repair and, in Settings and the toast, the launcher's own version.
- **The game** is named only as a fact: "Swords of Legends Online".
- **Credentials:** Fenji never asks for a Steam or Gameforge password, and says so where it matters: "Not your Steam password.", "Your Moonward account, not Steam.", "Sign in to Steam in its own window. Fenji never sees it."

### Motion
- **Easing:** an ease-out (`cubic-bezier(0.16, 1, 0.3, 1)`) for every change of state; an ease-in-out (`cubic-bezier(0.65, 0, 0.35, 1)`) for the two things that breathe.
- **Durations:** 120 ms for PLAY's press; 140 ms for a tip's fade; 160 ms for hovers, open states and a seal's menu; 200 ms for a scrim's fade and the switch; 220 ms for a popover or a dialog (a 4 px drop and 98.5% to full size as it fades in); 360 ms for the toast's 10 px rise; 400 to 700 ms for PLAY's look; 600 ms for a fill following its progress and for the kindled dissolve; 1200 ms for the ground's cross-fade. With no progress to show, PLAY's fill drifts (2.6 s each way) and the Game popover's bar sweeps a 30% block (1.8 s each way).
- **Reduced motion:** nothing animates or transitions. The ground and PLAY change at once, the drift rests at 36% and the bar at 30% from the left, the kindled moment is skipped, and the kindle's edge loses its glow.

### The host console
The same world as a terminal, all in Geist Mono 13 px with tabular numbers.
- **Title bar:** 32 px, ink-900, "fenji-server — Moonward" in text-2 500.
- **Status strip:** 40 px, ink-850, `role="status"`: the state word in its status colour ("online"), the world name in gold, then facts (version, uptime, "3 of 10 online", maintenance, ports) separated by faint " · ".
- **Log:** `role="log"`, anchored to the bottom, line-height 1.75, a three-column grid (64 px time, 48 px level, the message, 12 px gaps). Time in faint; INFO in muted; WARN in warn 500 and ERROR in danger 500, with those lines' messages raised to text colour. Player, character and world names in gold.
- **Command palette:** a square ink-850 list (no radius, no shadow) aligned with the prompt's text: 26 px rows of command and description; the selected row on ink-800.
- **Prompt row:** 48 px, ink-900 with a hairline above, a gold ">" prompt, the command in Geist Mono 14 px, a 1ch block cursor in text colour, and a faint key hint at the right: "Tab completes · ↑ ↓ choose · Enter runs". It is a combobox bound to the palette.

## Do's and Don'ts

### Do:
- **Do** keep one ember-600 action in view at most: PLAY when it's lit, otherwise the one primary action of a task, a popover section or a dialog. Under a dialog the 60% scrim sinks PLAY, so the dialog's ember is the one.
- **Do** give every state of the stage one line of state and at most one action beside PLAY, its cure: Stop, Cancel, Refresh, Retry, Paste new link, Check for updates, or the Game popover's next step.
- **Do** keep PLAY on the stage in every state, closed (ink-800, faint) when Play can't happen, with the reason in the state line or the meta line.
- **Do** say what is true in plain words beside every status colour: "Online · 3/10 players", "Maintenance · back around 21:00", "1 file needs repair".
- **Do** open Game, Account and Settings as 404 px popovers under their own controls, with the control shown open; keep Add a server, a key and Remove as centred 500 px dialogs, and an update as a corner toast.
- **Do** keep the window at 1200 × 720 with minimise and close only.
- **Do** ship Cinzel, Geist, Geist Mono and the 焚寂 subset of Noto Serif SC inside the product (OFL); load nothing from a font CDN.
- **Do** use the 2 px ember-500 focus ring with a 2 px offset on every focusable element, and keep text at 12.5 px or larger.
- **Do** say, wherever Steam or Gameforge comes up, that Fenji never asks for those passwords.
- **Do** keep PLAY closed off Linux and say so in one line: "Playing on Windows isn't supported yet."
- **Do** name the game only as a fact ("Swords of Legends Online") and draw all art in code.

### Don't:
- **Don't** use ember for anything but PLAY, the one primary action, the 焚 seal, the edges of the active seal, of + and of an open account, focus, the caret, selection and progress.
- **Don't** bring back tabs, a side column, a blade, key art or a news carousel: the stage holds one server and PLAY.
- **Don't** let anything but PLAY glow; no gradient text, glass blur, coloured stripes down the side of a card, popover or note, or hard offset shadows.
- **Don't** draw servers as round avatars with presence dots: they're square ink seals over a state bar.
- **Don't** use emoji, small labels above headings, or section numbers.
- **Don't** use Inter, Roboto or Arial, or any font, logo, screenshot or art from the game.
- **Don't** ask for, or offer a field for, a Steam or Gameforge password anywhere in Fenji.
- **Don't** add a maximise button or a resizable window.
- **Don't** show the game's build numbers or file totals: progress is a percentage.
- **Don't** let a status colour carry meaning without words, or put text below 12.5 px.
