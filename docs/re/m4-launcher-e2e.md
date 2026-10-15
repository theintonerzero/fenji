# M4 launcher: a new player from an invite link to the lobby, on bare metal

The desktop launcher's end-to-end run (launcher plan Task 16, #302; launcher spec §4 and §5): a player with nothing but an invite link and their own copy of the game. The steps are pasting the link, creating an account, choosing the copy, checking it, pressing Play, and reaching the lobby.

## Setup

The run used `scripts/spike/run-bare.sh run … --launcher-app` on the Linux game box, under every guard of CLAUDE.md's "Bare-metal client runs":
- **The server:** a local `fenji-server` with `tls = "self"`, bound to 127.0.0.1, with its store in the run's git-ignored workspace.
- **The network:** the client and the launcher shared one namespace whose network is loopback only.
- **The client:** umu-run in the dedicated, primed prefix, and the pinned copy, reflink-copied into the run.
- **The guards:** Steam not running (checked and watched), the AGT driver watchdog armed, and no `--gf-sdk` flag.

The launcher:
- **The build:** the unsigned AppImage from `cargo tauri build --bundles appimage`, unpacked into the run.
- **Its folders:** data, config and cache inside the run, so it started with no settings file.
- **Its tools:** GE-Proton11-7 through `PROTONPATH`, and umu-launcher 1.4.4.
- **Its prefix and runtime:** the dedicated prefix is bound where the launcher keeps its own, and umu's runtime where its umu-run looks for it.

The display and the inputs:
- **The display:** a dedicated rootful Xwayland, 1920×1080, with glamor on.
- **The inputs:** `--click`, `--type` and `--key` through `xclick.py` and `xkey.py`, with a screenshot after every input and every 30 s.
- **The password:** typed from `--password-file` through `xkey.py --type-file`, so it was never on a command line or in a file of the run.

GTK's folder dialog lists the run's copy as a bookmark, "SoLO (this run)". Choosing the folder is two clicks, because the dialog's completion mangles a typed path (rehearsals r3 and r4).

## The run

`m4-launcher-e2e`, 2026-10-15, under `--full-check`, for a new account, `player`, with a fresh invite. Times are seconds from the run's start; the launcher's own log gives the model's steps. Everything here is **Observed** in the run's screenshots and logs.

| # | Step | At (s) | What happened |
|---|---|---|---|
| 1 | The invite link | 15–28 | The welcome screen's field took the link with its pin, typed at 17 s. At Return (28 s) the launcher added the server, and every tab showed "Create your account". |
| 2 | The account | 32–49 | The name, the password and its confirmation, then Return. At 49 s the launcher logged "made an account with an invite" and "signed in". |
| 3 | The folder | 55–68 | The Client tab, then "Choose folder", the bookmark and Open. At 68 s the quick check found 210 files and none failing, and the screen read "Your copy is ready". |
| 4 | "Check every file" | 85–98 | Every file was hashed in about 14 s: at 90 s the blade stood at 22% (78 of 210 files), and at 98 s the check ended with 210 files and none failing. The screen then read "Every file checked today". |
| 5 | Play | 595–598 | The Play tab, then PLAY. Play's own quick check found 210 files and none failing, and the launcher logged "the game is running" at 598 s. |
| 6 | The lobby | 610–720 | The server had the account "in the lobby" at 610 s, 12 s after Play, and the client sent 402 a second later. By 630 s the client showed its title screen, "Press Any Key". A click at 673 s counted as the key, as in M3's m3-l2: 370, 496 and 292 at once, and the new account's empty list opened character creation (the 720 s screenshot). |
| 7 | The end | 780 | The timeout ended the run with the game still running (exit 124). |

**The watchdog's verdict: exit 0.** "No sign of driver activity in 1566 polls". The run added no driver service and no `.sys` file, in the prefix or anywhere else it watches (`services-diff.txt`, `sys-diff.txt` and `sys-elsewhere.txt` are empty). The prefix's new files are Proton's own: its DXVK, vrclient, OpenXR, nvcuda and Discord-bridge DLLs, refreshed as Proton starts. The client's logs and settings were written in the run's copy.

**Inferred:** the run used the launcher's own build and the player's flow. The harness supplied only the display, the inputs and the guards. On a player's own desktop two things differ:
- the network isn't limited to the server;
- umu-run downloads its runtime on its first start there, as umu-launcher does for anyone.

## Rehearsals and what they found

Each rehearsal ran under the same guards. The watchdog exited 0 in every one.

| Rehearsal | Reached | Found | Fixed in |
|---|---|---|---|
| r1 | The welcome screen | The clicks missed: the window is centred on the display, at (360, 180). | The run's click offset |
| r2 | The account form | A click and a typed line collided: each input starts its own Python, so a click due 1 s before typing can land after it. | Inputs spaced 2 s apart |
| r3, r4 | Choosing the folder | GTK's folder dialog mangles a typed path through its completion. | A GTK bookmark of the run's copy (`app_bookmark`) |
| (any) | All screens | The blade didn't draw in the desktop launcher: the shell's CSP refuses `style` attributes, and Vite's dev server sends no CSP, so the view's own checks missed it. | #325, fixed in #326 (inline styles through CSSOM) |
| r5 | Play | umu-run died 3 ms after starting: "Fatal Python error: Failed to import encodings module". The AppImage's AppRun and GTK hook point `PYTHONHOME`, `PYTHONPATH`, `PATH`, `LD_LIBRARY_PATH`, `XDG_DATA_DIRS` and more into the image, and every program the launcher started inherited them. | #329, fixed in #331 (`appimage::outside`) |
| r6 | Play | umu-run ran but found no runtime under the run's `XDG_DATA_HOME`, so it wanted to download one with no network. It then ended with 0, because umu-launcher 1.4.4 logs every exception and exits cleanly, and the launcher showed nothing. | The runner binds umu's runtime at the run's `data/umu`; #332, fixed in #333 (any end within 15 s is "couldn't start") |
| r7 | The lobby | The whole flow, through Play into the lobby: the server had tester5 "in the lobby" 12 s after Play. The client stayed on its title screen, which waits for a key. | The run clicks the title 75 s after PLAY (a click is the client's "any key", m3-l2) |

## Other observations

- While an invite is pending, every tab shows "Create your account". That is the route's design: the account comes first.
- GTK's folder dialog opens at the display's top left with the host's home folder. It's GTK's own dialog, so the bookmark is what makes the run's copy a two-click choice.
- The launcher writes `game.log` in its data folder's `logs` for each start. umu-run's and Proton's output there was where r5 and r6 were diagnosed.

## Evidence

This is all git-ignored, under `re/`:
- **The run:** `re/m4/launcher-e2e/` holds the run's evidence: its inputs, screenshots (`step-Ts-KIND.png`, `screen-Ns.png`), the watchdog's log and baseline, and the server's log.
- **The launcher's own logs:** `launcher.<date>.log` and `game.log`.
- **The rehearsals:** `re/bare-metal/out/m4-launcher-e2e-r1` to `-r7`.
