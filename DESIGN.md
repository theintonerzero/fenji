---
name: Fenji
description: Ember Sword, Fenji's one brand identity for the launcher, the host console, the website and the docs.
colors:
  ink-950: "#0c0c0f"
  ink-900: "#111115"
  ink-850: "#16161b"
  ink-800: "#1c1c22"
  line: "rgba(255, 255, 255, 0.07)"
  line-strong: "rgba(255, 255, 255, 0.12)"
  outline: "rgba(255, 255, 255, 0.16)"
  field-line: "#706b63"
  scrim: "rgba(6, 6, 8, 0.86)"
  text: "#f2efe9"
  text-prose: "#d8d2c8"
  text-2: "#c9c2b7"
  muted: "#a8a196"
  faint: "#8c867c"
  on-ember: "#ffffff"
  ember-600: "#c8431f"
  ember-500: "#e4572e"
  ember-300: "#ff7a45"
  ember-glow: "#ffb070"
  ember-tint: "#2a1712"
  gold-300: "#f2b365"
  gold-100: "#ffd9ad"
  brass: "#c9a36a"
  blade-steel: "#3a3a42"
  blade-grip: "#5a3b26"
  vignette-warm: "#2a1510"
  vignette-warm-mid: "#140d0c"
  vignette-cool: "#17171c"
  vignette-cool-mid: "#111114"
  ok: "#7fd38f"
  warn: "#e8b04a"
  danger: "#f26d7d"
  info: "#9cc3e4"
  info-tint: "#172028"
typography:
  wordmark:
    fontFamily: "Cinzel, serif"
    fontSize: "19px"
    fontWeight: 700
    letterSpacing: "0.26em"
  display:
    fontFamily: "Cinzel, serif"
    fontSize: "54px"
    fontWeight: 600
    lineHeight: 1.05
    letterSpacing: "0.03em"
  headline:
    fontFamily: "Cinzel, serif"
    fontSize: "45px"
    fontWeight: 600
    lineHeight: 1.1
    letterSpacing: "0.03em"
  section:
    fontFamily: "Cinzel, serif"
    fontSize: "26px"
    fontWeight: 600
    letterSpacing: "0.03em"
  dialog-title:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "21.6px"
    fontWeight: 600
    lineHeight: 1.3
  title:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "18px"
    fontWeight: 600
    lineHeight: 1.3
  status:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "15.5px"
    fontWeight: 400
  body:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.6
  nav:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 600
  secondary:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "13.5px"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "13.5px"
    fontWeight: 500
    lineHeight: 1.4
  data:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
  small:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "12.5px"
    fontWeight: 400
    lineHeight: 1.5
  play:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "17px"
    fontWeight: 700
    letterSpacing: "0.2em"
  button-primary:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 700
    letterSpacing: "0.02em"
  button-secondary:
    fontFamily: "Geist, system-ui, sans-serif"
    fontSize: "14.5px"
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
  pill: "18px"
  dot: "50%"
spacing:
  "4": "4px"
  "8": "8px"
  "12": "12px"
  "16": "16px"
  "20": "20px"
  "24": "24px"
  "32": "32px"
  "40": "40px"
  "48": "48px"
  "64": "64px"
components:
  header:
    backgroundColor: "{colors.ink-950}"
    height: "56px"
  tab:
    textColor: "{colors.muted}"
    typography: "{typography.nav}"
    height: "56px"
  tab-active:
    textColor: "{colors.text}"
    typography: "{typography.nav}"
    height: "56px"
  account-chip:
    textColor: "{colors.text}"
    rounded: "{rounded.pill}"
    height: "36px"
    padding: "0 12px 0 4px"
  window-control:
    textColor: "{colors.muted}"
    width: "46px"
    height: "32px"
  context-column:
    backgroundColor: "{colors.ink-900}"
    padding: "24px 22px"
    width: "340px"
  button-play:
    backgroundColor: "{colors.ember-600}"
    textColor: "{colors.on-ember}"
    typography: "{typography.play}"
    rounded: "{rounded.control}"
    width: "200px"
    height: "56px"
  button-play-disabled:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.muted}"
    typography: "{typography.play}"
    rounded: "{rounded.control}"
    width: "200px"
    height: "56px"
  button-primary:
    backgroundColor: "{colors.ember-600}"
    textColor: "{colors.on-ember}"
    typography: "{typography.button-primary}"
    rounded: "{rounded.control}"
    padding: "0 32px"
    height: "56px"
  button-secondary:
    textColor: "{colors.text}"
    typography: "{typography.button-secondary}"
    rounded: "{rounded.control}"
    padding: "0 22px"
    height: "56px"
  button-secondary-compact:
    textColor: "{colors.text}"
    typography: "{typography.button-secondary}"
    rounded: "{rounded.control}"
    padding: "0 18px"
    height: "44px"
  button-small:
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "32px"
  link:
    textColor: "{colors.gold-300}"
    typography: "{typography.button-secondary}"
  input:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.text}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 14px"
    height: "44px"
  card:
    backgroundColor: "{colors.ink-850}"
    textColor: "{colors.text}"
    rounded: "{rounded.card}"
    padding: "20px"
  avatar:
    backgroundColor: "{colors.ember-tint}"
    textColor: "{colors.gold-300}"
    rounded: "{rounded.dot}"
    size: "34px"
  tag:
    backgroundColor: "{colors.ink-800}"
    textColor: "{colors.muted}"
    typography: "{typography.small}"
    rounded: "{rounded.chip}"
    padding: "2px 8px"
  status-dot:
    rounded: "{rounded.dot}"
    size: "8px"
  progress-track:
    backgroundColor: "{colors.ink-800}"
    rounded: "2px"
    height: "4px"
  progress-fill:
    backgroundColor: "{colors.ember-500}"
    height: "4px"
  switch-on:
    backgroundColor: "{colors.gold-300}"
    rounded: "{rounded.card}"
    width: "40px"
    height: "24px"
  dialog:
    backgroundColor: "{colors.ink-850}"
    rounded: "{rounded.card}"
    padding: "28px 28px 24px"
    width: "480px"
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

**Creative North Star: "The Ember Sword"**

Fenji is named for 焚寂, the cursed sword, and the system is built around one blade whose ember light climbs as the player's own copy of the game is checked and made ready. Readiness is felt before it is read. Everything else is quiet ink: a near-black ground, tonal panels a step lighter, hairline borders, warm off-white text. Ember is rationed so hard that the eye always finds the one thing to do. The finish is that of a premium game launcher (familiar header-and-tabs structure, a single big Play button, a context column) dressed in xianxia restraint rather than key art and news carousels.

The density is calm: one fixed window, one primary action, plain sentences that say what is true about the server and the copy. Cinzel carries names and titles with an inscribed, classical voice; Geist runs the interface; Geist Mono carries data and the host's console, which is the same world rendered as a terminal. The key art is drawn in code (the blade, a vignette of light behind it) and never borrowed from the game.

These tokens are Fenji's single identity, approved by the owner on 2026-10-13, for the launcher, the host console, the website and the docs. The fonts (Cinzel, Geist, Geist Mono, and Noto Serif SC subset to 焚寂) are SIL Open Font License and ship inside each product; the launcher's content security policy is `default-src 'self'`, so nothing loads from a font CDN. The Google Fonts links on the mockups are for the mockups only.

**Key Characteristics:**
- Ink ground with tonal steps, hairline borders and almost no shadow.
- One ember action per screen; gold for names and links; status colours only for status.
- The blade is the progress indicator and the brand image at once.
- Cinzel for titles, Geist for the interface, Geist Mono for data and the console.
- A fixed 1200 × 720 window with a 56 px header, a main area and a 340 px context column.
- Plain, true sentences; never a request for platform credentials.

## Colors

A warm-neutral ink palette with a single ember voice, a gold accent for people and links, and four quiet status hues.

### Primary
- **Forge Ember** (ember-600): the fill of the one primary action on a screen (PLAY, Continue, Create account, Repair from Steam, Restart and update), always with white text (4.9:1). Also the seal tile behind 焚 in the wordmark.
- **Live Ember** (ember-500): the active tab's 2 px underline, the 2 px focus ring, the 4 px progress fill, text selection, and the cool end of the blade's gradient.
- **Blade Light** (ember-300) and **Ember Glow** (ember-glow): the blade's lit gradient and the bloom and motes at its tip. Never used on interface chrome.

### Secondary
- **Lantern Gold** (gold-300): names of people and servers' hosts (the "Wen:" prefix on a host notice, player names in the console), links and text buttons, avatar initials, and the on state of a switch. Gold marks people and places to go; it never marks the primary action.
- **Blade Tip** (gold-100): the hot tip of the blade, the sharp progress edge on the blade, and link hover.
- **Guard Brass** (brass): the sword's guard and pommel. Art only.

### Tertiary (status)
- **Jade OK** (ok): online, verified, files fine. Shown as an 8 px dot or a check, always beside words.
- **Amber Warn** (warn): maintenance, a file that needs repair, console WARN.
- **Cinnabar Danger** (danger): errors and unreachable servers, always as a circled-exclamation icon plus words; the error border on an invalid input; console ERROR.
- **Mist Info** (info): notes and help icons; with **Info Tint** (info-tint) as the ground of an informational badge or avatar.

### Neutral
- **Ink 950** (ink-950): the app ground, the header, the console log.
- **Ink 900** (ink-900): the context column, the console title bar and prompt row.
- **Ink 850** (ink-850): cards, dialogs, the console status strip and command palette.
- **Ink 800** (ink-800): inputs, the disabled PLAY, hover and the selected palette row, icon tiles and tags.
- **Line** (line, white 7%): hairlines, dividers, card borders, the header's bottom edge.
- **Line Strong** (line-strong, white 12%): input borders and the dialog border.
- **Outline** (outline, white 16%): secondary button borders and checkbox borders.
- **Field Line** (field-line): the border of inputs and fields: 3.2:1 against the ink-800 fill and 3.4:1 or more against the grounds around it, so a field's edge meets the 3:1 non-text floor. Added 2026-10-15 with the Servers boards, fixing the boards' input boundaries.
- **Paper** (text): titles and primary text (17:1 on ink-950).
- **Prose** (text-prose): running sentences in the main area: the host's notice, bullet lists, "What's new" (13:1). Used on every board but missing from the token sheet; recorded here because the build uses it consistently.
- **Parchment** (text-2): secondary text, field labels, the status line (11:1).
- **Ash** (muted): inactive tabs, meta lines, definition terms, disabled text (7.6:1).
- **Smoke** (faint): footnotes such as the launcher version, timestamps in the console, separators; 12.5 px and up only (5.4:1).
- **Ember Tint** (ember-tint): the ground of the player's own avatar and the host's avatar.
- **Scrim** (scrim): the backdrop behind a modal dialog.

### Named Rules
**The One Ember Rule.** Ember is for the one primary action on a screen, the active tab, the focus ring, progress, the blade and the seal behind 焚. Nothing else is ember: not icons, not headings, not borders, not switches.

**The Gold Is People Rule.** Gold marks names (hosts, players, characters) and the places a link goes. It is never a fill for an action.

**The Status Never Alone Rule.** A status colour always travels with words, and errors also carry an icon. A dot alone is never the message.

## Typography

**Display Font:** Cinzel (with serif)
**Body Font:** Geist (with system-ui, sans-serif)
**Label/Mono Font:** Geist Mono (with ui-monospace, monospace), tabular numbers
**Mark:** Noto Serif SC 700, subset to the two glyphs 焚寂

**Character:** Cinzel's inscribed capitals give server names and screen titles the weight of a carved plaque; Geist keeps the interface crisp and modern beneath it; Geist Mono makes data and the console line up.

The scale is a Minor Third (1.200) on a 15 px base: 12.5, 15, 18, 21.6, 25.9, 31.1, 37.3, 44.8, 53.7. Titles round to whole pixels (26, 31, 37, 45, 54). Secondary text sits half a step down, at 13.5 px. The shell also uses five named UI sizes: 13 px for data, 14 px for tabs and context-column headings, 14.5 px for secondary buttons and links, 15.5 px for the status line, and 17 px for PLAY.

### Hierarchy
- **Wordmark** (Cinzel 700, 19 px, tracked 0.26em, uppercase FENJI): beside the 26 px ember seal tile in the header and at the top of brand pages.
- **Display** (Cinzel 600, 54 px, line-height 1.05, tracked 0.03em): the server's name on the Play screens, one line, truncated with an ellipsis.
- **Headline** (Cinzel 600, 45 px, line-height 1.1): screen titles on task screens: "Welcome to Fenji", "Create your account", "Set up the game", "Settings".
- **Section** (Cinzel 600, 26 px): section headings on brand and documentation pages (the token sheet).
- **Dialog title** (Geist 600, 21.6 px, line-height 1.3).
- **Title** (Geist 600, 18 px, line-height 1.3): card titles, the blade's percentage readout, the account name.
- **Status** (Geist 400, 15.5 px, text-2): the line under the server name, led by a status dot or icon.
- **Body** (Geist 400, 15 px, line-height 1.6): sentences and input text; keep main-column prose to about 520 px.
- **Nav** (Geist 600, 14 px): tabs and context-column headings.
- **Secondary / Label** (Geist 400 or 500, 13.5 px): helper text, meta lines, field labels.
- **Data** (Geist 400, 13 px): definition lists in cards and the context column.
- **Small** (Geist 400, 12.5 px): footnotes, table headers, the launcher version. The floor for any readable text.
- **Mono** (Geist Mono 400 and 500, 13 px, line-height 1.75, tabular numbers): build numbers, invite codes, paths, and the whole host console (14 px on the prompt).

### Named Rules
**The Plaque Rule.** Cinzel is only for the wordmark, server names and screen titles. Body, buttons, labels and tabs are Geist.

**The Tracked Caps Rule.** PLAY is the only tracked uppercase button label (0.2em). Every other button is sentence case, 0.02em at most.

## Layout

The launcher is a fixed 1200 × 720 window: no resizing, no maximise, minimise and close only. A 56 px header runs across the top. Below it, a 860 px main area and a 340 px context column (ink-900, a hairline on its left edge, 24 px by 22 px padding, 22 px between sections, the launcher version pinned to its foot).

The main area has two compositions:
- **Hero** (Play, its states, Welcome, client setup, downloading, checking, repair): the blade stands at x 150 on the left; text starts at x 250, 120 px down (96 px on Welcome), at most 520 to 562 px wide. Order: title, status line, host notice, 34 px gap, the button row (12 px apart), a meta line below.
- **Page** (Server, Account, Sign in, Redeem, Settings): 48 px top and 64 px side padding, a 45 px title, then cards and forms up to 420 to 640 px wide; Settings uses a two-column card grid with 20 px gaps.

Spacing runs on 4, 8, 12, 16, 20, 24, 32, 40, 48 and 64. The boards also use a few in-between values that the token sheet does not list (22 px in the context column, 9 and 14 px gaps, 34 px above the button row); keep them where they sit and do not spread them.

The host console fills its terminal: a 32 px title bar, a 40 px status strip, the log filling the middle and anchored to the bottom, an optional command palette, and a 48 px prompt row. 24 px side padding throughout.

## Elevation & Depth

Flat by default, layered by tone. Depth comes from stepping up the ink ramp (950 ground, 900 column, 850 cards, 800 inputs) with a white 7% hairline at each edge. Only two things cast a shadow, and both are ambient, never coloured and never offset hard.

### Shadow Vocabulary
- **Primary lift** (`box-shadow: 0 10px 26px rgba(0, 0, 0, 0.5)`): the ember primary button, so the one action sits above the page.
- **Dialog** (`box-shadow: 0 24px 60px rgba(0, 0, 0, 0.6)`): modal dialogs over the scrim.

The only light in the system is the blade's own: a radial bloom at its tip and a vignette of warm or cool light behind it, drawn as gradients in the art, not as shadows on interface elements.

### Named Rules
**The Hairline Rule.** Cards and panels separate by tone and a 1 px white-7% border, never by shadow.

## Shapes

Gently rounded and consistent: 6 px for tags, the seal tile and checkboxes; 8 px for buttons, inputs and icon tiles; 12 px for cards, dialogs and the switch track; 18 px for the account pill; full circles for avatars and status dots. The 4 px progress bar uses 2 px ends. The host console's command palette is square (0 radius), as a terminal is.

Lines are 1 px everywhere except the active tab's 2 px ember underline and the 2 px focus ring. Icons are 1.8 px outline strokes on a 24 px grid, round caps, drawn at 13 to 20 px, coloured by `currentColor` or a status hue.

## Components

### Header and tabs
- **Structure:** 56 px tall on ink-950 with a hairline below; 24 px left inset. The seal (a 26 px ember-600 tile, 6 px radius, white 焚 in Noto Serif SC 14 px) and the FENJI wordmark, then the tabs 48 px to the right: Play, Server, Account, Client, 28 px apart.
- **Tabs:** Nav type; inactive in muted; the active tab in text with a 2 px ember-500 underline at the header's bottom edge. When Settings is open no tab is active and the Settings button takes an ink-800 tile with text colour.
- **Right side:** the account pill (36 px, 18 px radius, a 28 px avatar, the username in 13.5 px 600), then Settings, Minimise and Close as 46 × 32 icon buttons in muted. There is no maximise button.

### Buttons
- **PLAY:** 200 × 56, ember-600, white, Geist 700 17 px tracked 0.2em, 8 px radius, primary lift. At most one per screen.
- **PLAY disabled:** the same box in ink-800 with muted text and no shadow; the reason is always written beside or below it. While the game runs it reads "Running" with an ok dot.
- **Primary (worded):** 56 px tall, at least 200 px wide, 32 px side padding, ember-600, Geist 700 16 px, sentence case, primary lift. Used when the screen's one action is not Play.
- **Secondary:** 56 px tall beside a primary (44 px compact in cards and forms, 32 px small for "Change"), transparent with a 1 px outline border, Geist 600 14.5 px.
- **Link / text button:** gold-300, Geist 600 14.5 px (13.5 px inline), no underline in action rows, underlined with a 3 px offset inside sentences; gold-100 on hover.

### Inputs / Fields
- **Style:** 44 px tall, ink-800 fill, 1 px field-line border, 8 px radius, 14 px padding, Body type in text colour. Paths, addresses being typed and readonly locations use Geist Mono 14 px.
- **Label:** always visible above, 13.5 px 500 in text-2, 8 px gap.
- **Boundary:** the field-line border meets the 3:1 non-text floor against the fill and every ground. The approved boards of 2026-10-13 drew it in line-strong (about 1.15:1 against the fill); the build uses field-line, as the Add server board shows.
- **Focus:** a 2 px ember-500 outline with a 2 px offset.
- **Error:** a 1 px danger border, `aria-invalid`, and a message below in danger with the circled-exclamation icon, written as what happened and what to try.
- **Checkbox:** 20 px, 6 px radius, outline border on ink-800, with its label clickable. **Switch:** 40 × 24 track, gold-300 when on, an ink-950 18 px knob; a real checkbox with `role="switch"` underneath.

### Cards / Containers
- **Corner Style:** 12 px.
- **Background:** ink-850, with a 1 px line border; no shadow.
- **Internal Padding:** 20 px (16 px in the context column, 14 by 16 px for a compact server row).
- **Contents:** an 18 px title, often led by a 36 px ink-800 icon tile with a gold icon; definition lists in Data type with muted terms; result tables with 12.5 px 600 muted headers and Geist Mono file paths.

### Lists
- **People:** 34 px circular avatars with initials (12 px 700), a 14 px 600 name, and a 12.5 px muted status line, 12 px apart. The host and the player sit on ember-tint with gold initials; other players rotate through tinted grounds.
- **Bullets:** 6 px faint dots, Prose type, 10 to 12 px apart.
- **Action rows:** 40 px rows with hairline tops, an ok check, the item in 15 px Prose and a gold "Tab name ›" link at the right.

### Server rows
The Servers tab (the boards `Servers` and `AddServer`) lists the player's servers in one ink-850 list: 12 px radius, a 1 px line border, rows split by hairlines, 14 by 16 px padding, in the order they were added.
- **Anatomy:** a 40 px ink-950 tile (8 px radius, a line border) with the server's initial in Cinzel 600 18 px; the name in Cinzel 600 22 px, with the address beside it in 12.5 px muted (a lock and "own key" when the server is pinned); the status line in 13.5 px text-2 with its dot, the ping in tabular numerals; and the host's notice on one line, led by the host's name in gold.
- **Selected:** the server Play opens. Ink-800 with a 1 px line-strong inset outline (11 px top corners, inside the list's 12), and a compact PLAY on the right: 44 px tall, at least 128 px wide, Geist 700 15 px tracked 0.2em, ember-600 with the primary lift. It is the page's one ember action. A selected server that can't take players shows no PLAY; its status line says why.
- **Other rows:** no button, and ink-800 on hover. A click selects.
- **Title row:** the page title "Servers" with one line under it ("The server you choose is the one Play opens."), then Refresh (a gold text button with its icon) and Add server (secondary, 44 px) on the right.
- **Context column:** the selected server's name as its heading, a facts card (Address, Version, Players, Ping, Hosted by, then a gold "Source code" link), Playing now, and "Remove <name> from the list" (a text button in text-2 with its icon) above the launcher's version.

### Status
- **Status line:** an 8 px dot (ok, warn, faint for no server or while checking) or, for errors and an unreachable server, a 16 px danger icon (a Servers row keeps the faint dot for an unreachable server, as the Servers board draws it), then plain words separated by " · " in Status type: "Online · 3 friends in the lobby · hosted by Wen", "In maintenance · back around 21:00 · hosted by Wen", "Can't reach Moonward · last seen today at 18:42".
- **Host notice:** Prose type, led by the host's name in gold 600 and a colon.
- **Meta line:** 13.5 px muted with a 13 to 16 px icon: "Client verified · build 10015727 · 210 files", "Last checked at 20:14 · Play opens when Wen ends maintenance".

### Progress
- **The blade** shows progress wherever readiness is the subject (downloading, checking, repair), with a right-aligned readout to its left: an 18 px 600 percentage or count and a 12.5 px muted line, exposed as `role="progressbar"` with a written `aria-valuetext`.
- **The bar:** 4 px tall, ink-800 track, ember-500 fill, 2 px radius, with a 12.5 px label row above (what, and a Geist Mono count on the right). Only where the blade isn't shown.

### The context column
Ink-900, 340 px, 24 by 22 px padding, sections 22 px apart with 14 px 600 headings: Playing now (who's on the server; the boards of 2026-10-13 say "In the lobby", true only before the world), Server (a card of facts), Your client, Host notice, What's next, About Fenji. It explains state when the main area cannot: "The lobby is closed while Moonward is in maintenance." The launcher version sits at the foot in 12.5 px faint.

### Dialogs
480 px wide, ink-850, a 1 px line-strong border, 12 px radius, 28 px padding (24 px at the foot), the dialog shadow, over the scrim; `role="dialog"` with `aria-modal`. A 40 px icon tile and a 21.6 px title, a 15 px description, hairline-divided content, and the actions bottom right: secondary first, then the primary.
- **Add a server** (the board `AddServer`): the 40 px tile with a gold servers icon, "Add a server", one line on what to paste, a hairline, the field "Address or invite link" in Geist Mono with its error below it, then Cancel and Add server (the one ember).

### The blade (signature component)
A single vertical sword drawn in vector: a steel blade (blade-steel), a brass guard and pommel, and a leather-dark grip (blade-grip). Its states:
- **Unlit:** steel only. No ready copy yet (first run, client setup), or a copy this system can't start.
- **Climbing:** an ember gradient (ember-500 at 15% to ember-300 to gold-100) clipped at the progress height, with a sharp 7 × 1 px gold-100 tick at the progress point. Downloading, checking, repairing.
- **Lit:** the full gradient. The copy is verified.
- **Lit with bloom:** the full gradient plus the radial tip bloom (ember-glow to transparent) and a few ember motes. The copy is ready and Play is open (or the game is running).

**The Blade And Ground Rule.** The blade shows the player's copy; the background shows the server. Behind the blade, a radial vignette from the top left: warm (vignette-warm to vignette-warm-mid to ink-950) when the server is up and open, cool (vignette-cool to vignette-cool-mid to ink-950) when there is no server, it is in maintenance, or it can't be reached.

**The Readiness Only Rule.** Use the blade only where readiness is the subject. It is not decoration for pages about accounts, servers or settings.

### The host console
The same world as a terminal, all in Geist Mono 13 px with tabular numbers.
- **Title bar:** 32 px, ink-900, "fenji-server — Moonward" in text-2 500.
- **Status strip:** 40 px, ink-850, `role="status"`: the state word in its status colour ("online"), the world name in gold, then facts (version, uptime, "3 of 10 online", maintenance, ports) separated by faint " · ".
- **Log:** `role="log"`, anchored to the bottom, line-height 1.75, a three-column grid (64 px time, 48 px level, the message, 12 px gaps). Time in faint; INFO in muted; WARN in warn 500 and ERROR in danger 500, with those lines' messages raised to text colour. Player, character and world names in gold.
- **Command palette:** a square ink-850 list (no radius, no shadow) aligned with the prompt's text: 26 px rows of command and description; the selected row on ink-800.
- **Prompt row:** 48 px, ink-900 with a hairline above, a gold ">" prompt, the command in Geist Mono 14 px, a 1ch block cursor in text colour, and a faint key hint at the right: "Tab completes · ↑ ↓ choose · Enter runs". It is a combobox bound to the palette.

## Do's and Don'ts

### Do:
- **Do** keep one ember-600 action per screen; with no Play, give the screen's one task a worded ember button (Continue, Create account, Repair from Steam).
- **Do** say what is true in plain words beside every status colour: "Online · 3 friends in the lobby", "1 needs repair", "Can't reach Moonward".
- **Do** keep PLAY visible but disabled (ink-800, muted) while the copy is being readied, the server is in maintenance or the system can't start the client, and write the reason beside it; when the server can't be reached, the one action becomes a worded "Try again".
- **Do** keep the window at 1200 × 720 with minimise and close only.
- **Do** ship Cinzel, Geist, Geist Mono and the 焚寂 subset of Noto Serif SC inside the product (OFL); load nothing from a font CDN.
- **Do** use a 2 px ember-500 focus ring with a 2 px offset on every focusable element, and keep text at 12.5 px or larger.
- **Do** say, wherever Steam or Gameforge comes up, that Fenji never asks for those passwords: "Fenji never asks for your Steam or Gameforge password." and "You sign in to Steam in DepotDownloader's own window, never in Fenji."
- **Do** use exactly "Fenji can't start the client on this system yet" off Linux, keep PLAY disabled, and list what still works there (create an account, check the game folder, see the server's state).
- **Do** name the game only as a fact ("Swords of Legends Online") and draw all art in code.

### Don't:
- **Don't** use ember for anything but the primary action, the active tab, focus, progress, the blade and the seal.
- **Don't** use glow shadows on interface elements, gradient text, glass blur, coloured side borders, or hard offset shadows. The blade's own light is the only glow.
- **Don't** use emoji, small labels above headings, or section numbers.
- **Don't** use Inter, Roboto or Arial, or any font, logo, screenshot or art from the game.
- **Don't** ask for, or offer a field for, a Steam or Gameforge password anywhere in Fenji.
- **Don't** add a maximise button, a resizable window, a news carousel, or key art competing with Play.
- **Don't** let a status dot carry meaning without words, or put faint text below 12.5 px.
