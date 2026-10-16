# The launcher's boards

The owner approved these boards on 2026-10-13: the "Ember Sword" direction, the seventh version of its design canvas. The two Servers boards were added on 2026-10-15 in the same language, under the owner's pre-approval of that day, for the server list the hosting spec's HA6 asks for. Each is a 1200 × 720 HTML file, one per state of the launcher, plus the host's console and the brand-token sheet. [`DESIGN.md`](../../../DESIGN.md) at the repository's root is the design system they share. The launcher's spec, [`2026-10-15-m4-launcher-design.md`](../../superpowers/specs/2026-10-15-m4-launcher-design.md), says what the launcher builds from them and which three gaps it fixes.

Open a board in a browser to see it. The boards were drawn in a design canvas:
- the `<x-dc>` and `<helmet>` wrappers, `support.js` and the script at the end belong to that tool and do nothing here;
- the fonts load from Google Fonts for viewing only. The launcher ships its own copies (DESIGN.md).

| Board | State |
|---|---|
| [`EmberSword.dc.html`](boards/EmberSword.dc.html) | Play · ready |
| [`Playing.dc.html`](boards/Playing.dc.html) | Play · game running |
| [`Maintenance.dc.html`](boards/Maintenance.dc.html) | Play · server in maintenance |
| [`Offline.dc.html`](boards/Offline.dc.html) | Play · server unreachable |
| [`Unsupported.dc.html`](boards/Unsupported.dc.html) | Play · Windows and macOS |
| [`Welcome.dc.html`](boards/Welcome.dc.html) | Welcome · first run |
| [`Redeem.dc.html`](boards/Redeem.dc.html) | Account · create from an invite |
| [`SignIn.dc.html`](boards/SignIn.dc.html) | Account · sign in |
| [`ServerTab.dc.html`](boards/ServerTab.dc.html) | Server · details and address (superseded by `Servers.dc.html`) |
| [`Servers.dc.html`](boards/Servers.dc.html) | Servers · the list (added 2026-10-15, launcher plan Task 1) |
| [`AddServer.dc.html`](boards/AddServer.dc.html) | Servers · the Add server dialog, with a refusal (added 2026-10-15) |
| [`AccountTab.dc.html`](boards/AccountTab.dc.html) | Account · signed in |
| [`ClientSetup.dc.html`](boards/ClientSetup.dc.html) | Client · set up the game |
| [`Downloading.dc.html`](boards/Downloading.dc.html) | Client · downloading from Steam |
| [`Checking.dc.html`](boards/Checking.dc.html) | Play · checking files |
| [`Repair.dc.html`](boards/Repair.dc.html) | Client · one file needs repair |
| [`Settings.dc.html`](boards/Settings.dc.html) | Settings |
| [`Update.dc.html`](boards/Update.dc.html) | Launcher update ready |
| [`HostConsole.dc.html`](boards/HostConsole.dc.html) | Host · the `fenji-server` console |
| [`BrandTokens.dc.html`](boards/BrandTokens.dc.html) | Brand · the Ember Sword tokens |

The names on the boards ("Moonward", "Wen", "lin_yue") are made up. The key art is drawn in code, and nothing on the boards comes from the game (ground rule 3).
