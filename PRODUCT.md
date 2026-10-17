# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

- **The launcher:** a Tauri 2 desktop app, TypeScript and Vite with no UI framework (M3 plan Task 11), over the Rust library `crates/fenji-launcher`. Its content security policy is `default-src 'self'`, so fonts and images ship inside the app and nothing loads from a CDN.
- **The host console:** `fenji-server` itself (Rust), in a terminal.

## Users

- **Players** are friends a host has invited. Each owns the game legitimately: Steam app 1418100 already in their library, or a copy from Gameforge's own launcher when it was distributed. Many aren't technical. They want to sign in and play again with their friends. Linux comes first; Windows waits on the owner's decision about the client's AGT driver, and macOS has no tested way yet.
- **Hosts** are anyone who runs `fenji-server` for a small group of friends, on their own PC (Windows, Linux or macOS) or on a VPS over SSH. They start the server, invite friends, see who is on and put the server into maintenance. They do it from a console that shows live logs and takes commands, as a Minecraft server's console does.

## Product Purpose

Swords of Legends Online's official servers shut down on 2023-06-30. Fenji is a clean-room private server, with a launcher, that lets a small group of friends play the game again on a server one of them hosts.

Success looks like this. A host starts the server and sends invites. Each friend then gets into the lobby with their characters in minutes, without help: they redeem the invite, get their own copy of the game ready and press Play.

## Positioning

Fenji is clean-room and legitimate by construction. It uses no leaked server files, ships no game files, and only ever starts the player's own unmodified client. It is private, invite-only and non-commercial: no store, no payments, no telemetry. Anyone can host, and the code is public under AGPL-3.0-or-later.

## Operating Context

- **A player's first run:**
  1. They get an invite link from their host, open the launcher and redeem the invite with a username and password.
  2. They point the launcher at their game folder. A Steam owner can instead have it fetch the pinned build from their own Steam account through DepotDownloader; they sign in to Steam by scanning the code the launcher draws with the Steam Mobile App, or with their password, by running the command the launcher shows in their own terminal, never in Fenji.
  3. The launcher verifies the files, repairing them through DepotDownloader for Steam owners.
  4. They press Play. The launcher signs in, takes a single-use ticket and starts the unmodified client through umu-launcher and Proton on Linux.
- **Where the server runs:** on the same PC, on the LAN or on the internet. The launcher works without internet when the server is local, and update checks then fail quietly.
- **When the server can't take players:** it may be down or in maintenance. The launcher says so, with the host's message, and doesn't offer Play.
- **The host's console:** a start script per OS opens `fenji-server` in a terminal, with live logs above and a command line below: help, invites, who is online, kick, maintenance on or off with a message, and stop. Over SSH on a VPS it is the same console.

## Capabilities and Constraints

- **The launcher:**
  - **Servers:** a list, as on Minecraft's multiplayer screen, each added by address or invite link, with its name, the host's notice, players, state and ping.
  - **Account:** redeem an invite, or sign in.
  - **Client:**
    - the game folder, with quick and full verification and per-file results;
    - download and repair of the pinned build through DepotDownloader, for Steam owners;
    - a ready state.
  - **Play:** on Linux, through umu-run.
  - **Server status:** online, in maintenance with the host's message, or offline.
  - **Updates:** the launcher updates itself.
- **The window:** one fixed size, not resizable, with close and minimise only (no maximise).
- **Never:**
  - handle Steam or Gameforge credentials (ground rule 7);
  - download or ship client files from Fenji (ground rule 1);
  - change the client (ground rule 6);
  - start the client on Windows before the owner decides (gate #114, plan Amendment B);
  - send telemetry.
- **No game art:** no logos, screenshots or fonts from the game; only Fenji's own mark (ground rule 3; trademarks). The game may be named as a fact, e.g. "for Swords of Legends Online".
- **Open decisions:**
  - The launcher's self-update channel and signing key: settled in the launcher's spec (2026-10-15, L5): this repository's GitHub Releases, signed with Fenji's own update key.
  - Languages: English only for now.
  - Windows play: held back.

## Brand Commitments

- **Name:** Fenji, from 焚寂, the cursed sword of Gu Jian Qi Tan.
- **One brand identity:** the launcher's tokens become Fenji's tokens for the console, the docs and every other surface.
- **Fonts:** open-source and uncommon. Cinzel and Cormorant Garamond were offered as a starting idea to experiment from, not as a commitment.
- **Type scale:** 1.200 (Minor Third).
- **Tone:** a small private server for friends, not a storefront.
- **The launcher's bar (the owner, 2026-10-13):** familiar game-launcher conventions done brilliantly, at the finish of HoYoPlay, Riot Client and Battle.net. Premium and elegant, xianxia fantasy, clean and modern. The owner turned down a first set of directions (apothecary cabinet, ink wash and seal, ticket wallet, warm consumer app, and a plain category standard).
- **Key art:** drawn in code (vector peaks, a sea of clouds, the 焚寂 sword), never the game's art and never generated images.

## Evidence on Hand

None yet: no logo or mark, no screenshots of Fenji, no testimonials, no player counts. The game's own art, logos and screenshots must not stand in for any of it.

## Product Principles

1. **Legitimate by construction.** Every flow assumes the player's own copy and never touches their platform credentials.
2. **One button when ready.** Everything before Play exists to make Play certain.
3. **Say what is true.** The launcher reports the server's and the client's real state in plain words: verified, missing files, maintenance.
4. **Private and quiet.** No telemetry, nothing sent that isn't needed, and it works on a LAN.
5. **Hosting is part of playing.** Running a server should feel like starting a game server, not like administering one.
