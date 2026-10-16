# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.1] - 2026-10-16

### Fixed

- **server:** Hosting's follow-ups, the terminal, panics and versions (#366)
- **launcher:** The final review's deferred minors (#364)
- **deploy:** VPS units in Linux archives, plainer redeem words, no space in a host (#365)

### Documentation

- **docs:** The plan for M4's follow-ups, in 0.5.x releases (#362)

## [0.5.0] - 2026-10-16

### Added

- **store:** Save where characters stand, and create them at level 37 (#206)
- **server:** Add the [world] settings and [server] tick_hz to fenji.toml (#205)
- **proto:** Never send 68 or 714, and say what joining a server trusts (#209)
- **server:** Add the world's ECS core, its tick thread and its saves (#210)
- **server:** Show players to each other within the world's interest grid (#211)
- **server:** Check moves against the speed rule and relay them to viewers (#213)
- **server:** Route nearby, world and whispered chat with limits and a rate (#214)
- **server:** Add the admin commands, and invites shared with the CLI (#216)
- **proto:** The world's entering messages (#217)
- **server:** Enter the world from the lobby (#218)
- **data:** Add fenji-oodle with Oodle's stream headers and the chunk walk (#239)
- **tools:** Count the client's Oodle chunks by decoder and mode (#242)
- **server:** Players see each other in the world (#241)
- **tools:** Run two clients in one namespace (#240)
- **tools:** Hold a key down in a client run (#263)
- **server:** Move in the world, checked and relayed (#261)
- **tools:** Run sandboxed clients on the GPU's render node (#267)
- **data:** Decode Oodle Kraken in fenji-oodle (#262)
- **server:** Back up the database while the server runs (#271)
- **proto:** Chat's messages (#273)
- **server:** Chat and admin commands in the world (#274)
- **server:** Write a config on the first run, and add init (#275)
- **data:** Decode Oodle archives natively, and verify them against the client's DLL (#276)
- **server:** Server facts, MOTD, maintenance, ops and bans in the account API (#279)
- **launcher:** Checks report progress and stop when asked (#305)
- **launcher:** Fetch DepotDownloader and umu-launcher on first use, pinned and checked (#306)
- **launcher:** Run DepotDownloader in its own terminal, signed in by QR code (#307)
- **server:** Serve the account API with the server's own key, and pin it in invite links (#286)
- **launcher:** Trust a server's own key through the pin in its links (#309)
- **launcher:** Probe a server for its state, its key and its ping (#311)
- **server:** Name the server's host in its facts, for the launcher (#312)
- **launcher:** A list of servers in the settings, and what the Add dialog takes (#310)
- **launcher:** The launcher's model: servers, accounts, probes and snapshots (#313)
- **launcher:** The model checks, downloads and repairs the player's copy (#315)
- **launcher:** Play from the model, with the session it holds (#316)
- **launcher:** The Tauri shell, the app's skeleton and its CI job (#318)
- **launcher:** The view's tokens, fonts, components, blade and router (#320)
- **launcher:** The Welcome, account and Play screens (#322)
- **launcher:** The Servers, client and Settings screens, and the update dialog (#324)
- **server:** Add fenji-console, the host console's front ends (#319)
- **server:** One log hub for the console, files and stderr (#327)
- **deploy:** Release archives with start scripts for Linux, Windows and macOS (#321)
- **server:** Parse the host console's commands (#330)
- **launcher:** Signed self-updates, and the launcher's bundles in releases (#340)
- **server:** Let the host console run the world's commands (#342)
- **deploy:** The launcher's bundles carry their third-party notices (#345)
- **server:** The host console, in its own terminal or as plain lines (#346)
- **server:** Attach the host console to a running server (#350)

### Changed

- **deps:** Bump rustls to 0.23.46 for RUSTSEC-2026-0346 to 0349 (#264)

### Fixed

- **server:** Close a replaced connection within the caps (#204)
- **store:** Name an io error the database file's, not its permissions' (#208)
- **tools:** The v0.4.1 review's follow-ups for the runners and the watchdog (#207)
- **tools:** The watchdog sees a driver service re-pointed with ChangeServiceConfig (#215)
- **server:** Harden chat, admin commands and movement from the task reviews (#221)
- **server:** End the world cleanly on shutdown, panics and stalls (#224)
- **tools:** Hold PR titles to what fits beside GitHub's (#N) suffix (#260)
- **tools:** A read-only sandbox root and /dev, and --gpu's checks tightened (#268)
- **server:** Echo the client's transport time syncs, so its ping is real (#270)
- **server:** The world's final-review fixes (#283)
- **data:** The Oodle decoder's whole-plan review fixes (#317)
- **launcher:** Set inline styles through CSSOM, which the shell's CSP allows (#326)
- **launcher:** Keep the AppImage's environment from what the launcher starts (#331)
- **launcher:** Say the game couldn't start when umu-run ends at once with 0 (#333)
- **deploy:** Real copyright lines in the release notices, and a check for placeholders (#343)
- **launcher:** The final review's fixes: Later, the download folder, the release key (#347)
- **tools:** The two-client input check takes client 1's 1 s inputs in either order (#349)
- **tools:** The check on client 2's end allows the runner's whole seconds (#355)
- **server:** The hosting final review's fixes (#356)
- **launcher:** A link pasted from a wrapped line reads as one (#358)

### Documentation

- Add the M4 spec: the world, hosting and the launcher, in order, and what they share (#177)
- Add the M4 world spec, and use ! for admin commands since the client keeps / lines (#179)
- Add the M4 world implementation plan, and correct the outfit source (#182)
- **proto:** Read entering the world in the exe (Amendment A) (#212)
- **proto:** Read how players see each other (Amendment B) (#223)
- Allow patching the client, as long as it only ever talks to Fenji (#226)
- Add the open Oodle decoder spec (#227)
- Add the open Oodle decoder implementation plan (#228)
- **proto:** Read movement (Amendment C) (#238)
- Add the M4 hosting spec (host console, maintenance, TLS without a domain) (#245)
- Add the M4 hosting implementation plan (#246)
- Amend the hosting spec and plan to make hosting as easy as Minecraft (#272)
- **server:** The world's measurement and records (#278)
- **re:** Every Oodle block in the pinned client decodes natively, matching its DLL (#284)
- **launcher:** The desktop launcher's spec and plan, with the approved boards and DESIGN.md (#285)
- **launcher:** The Servers board and the Add server dialog (#304)
- **re:** Bring the docs up to date with client patching and fenji-oodle (#335)
- **launcher:** The launcher as shipped, in DESIGN.md, the privacy page and the README (#344)
- **release:** Signing a release waits for the owner's approval (#352)
- **deploy:** The write-up for hosts, with systemd, Caddy and backups (#351)
- **re:** M4's summary, and each piece's status in its spec (#360)

## [0.4.1] - 2026-10-13

### Fixed

- **store:** Keep the database owner-only, and overwrite deleted rows (#152)
- **server:** End a deleted account's session, and close when the client quits (#153)
- **server:** Build invite links from the account API's address, not the gate's (#154)
- **proto:** Refuse a base face or outfit in a create request that isn't a number (#155)
- **launcher:** Tell a rate-limited player how long to wait, and test the grant checks (#156)
- **server:** Keep one live game token per account, and make the dummy hash at start (#157)
- **server:** Let a reply blocked on a stuck client give way when the account logs in again (#159)
- **tools:** Close the watchdog's S3 gaps for NULL display names and user-mode LoadDriver (#162)
- **tools:** Scan the bare namespace's own tmpfs mounts, and name the prefix to the watchdog (#163)
- **tools:** Refuse bare-run paths the namespace's tmpfs would hide, and resolve HOME (#164)
- **tools:** Hide IBus's sockets from the bare namespace, and name the audio daemon as the residual (#166)
- **tools:** Refuse sandbox binds on the kernel's own filesystems wherever they are mounted (#167)
- **tools:** Mark a bare prefix that saw a gate event, and refuse it until it is rebuilt (#171)
- **store:** Empty the write-ahead log after an account delete, so no old copy lingers (#172)
- **tools:** Create the gate-event mark exclusively, so a planted FIFO can't stall the runner (#173)

### Documentation

- Say what deleting an account removes, and bring the stale M3 lines up to date (#161)
- **proto:** Say where the pickle decoder differs from the client's; pin a compressed table count (#165)

## [0.4.0] - 2026-10-13

### Added

- **auth:** Add fenji-auth: password hashes, tokens, invite codes, rules and the limiter (#129)
- **proto:** Type the lobby's messages and check what clients ask to store (#131)
- **store:** Add fenji-store: accounts, invites, tokens and characters in SQLite (#130)
- **server:** Add fenji-server with fenji.toml, the account API and admin commands (#132)
- **server:** Add the gate and the lobby: tickets, limits, list, create and delete (#133)
- **launcher:** Add fenji-launcher and fenji-launch: sign-in, verification, the launch spec (#134)
- **tools:** Let client runs use fenji-server, the launcher's ticket and key presses (#136)
- **launcher:** Let fenji-launch play start the client on Linux, as a player's launcher does (#140)
- **tools:** Add run-bare.sh, the guarded bare-metal client runner (#144)

### Fixed

- **tools:** Find the client and its winedevice when Wine renames them after the watchdog first saw them (#143)
- **tools:** Let the stub keep no auth capture, and end --work-pad in WORK (#145)

### Documentation

- Add the M3 (login and lobby) design and plan (#128)
- **re:** Read the client's lobby and platform paths (#135)
- Record the owner's decision on gate #114 and the guarded bare-metal client runs (#137)
- **re:** Run the lobby against the real client: list, create, restart, delete (#141)
- Record M3: the summary, the spec's M3 notes, and what changes for players and hosts (#149)

## [0.3.1] - 2026-10-12

### Added

- **tools:** Refuse session links in commits and PR descriptions (#109)

### Changed

- **deps:** Use as_chunks for constant-size chunks (#97)

### Fixed

- **tools:** Record the stub's arguments and check it started (#99)
- **tools:** Dump checks the cache against its key (#100)
- **tools:** Close the repo-guard follow-ups (#103)
- **proto:** Read every pickle form the client writes, and bound what a peer can send (#104)
- **proto:** Make the client simulator behave like the real client where it didn't (#105)
- **re:** Close the sandbox follow-ups carried since Caphras (#107)
- **proto:** Refuse a pickle header that reads as compressed, and NaN keys (#110)

### Documentation

- Add the public-facing community files (#96)
- **re:** Correct and link the M2 records (#106)
- **re:** Reject the server package found in a working copy (#112)

## [0.3.0] - 2026-10-12

### Added

- **proto:** Add fenji-proto with the inner header and the stored codec (#74)
- **proto:** Add frame keys and frames for both directions (#75)
- **proto:** Build and check the hello, and add the AES-128 variant (#76)
- **proto:** Add the server handshake and a client simulator (#77)
- **tools:** Let fenji-stub speak the client's handshake (#78)
- **proto:** Decode the client's first packets after auth (#81)
- **data:** Decode Oodle blocks with the client's DLL in a sandboxed helper (#82)
- **data:** Decode _index and map paths to archive records (#83)
- **data:** Read the client's tables and dump them (#85)

### Fixed

- **tools:** Keep M2's run artefacts out of git (#87)

### Documentation

- **re:** The launcher's login path, with an English interface (#55)
- Add the M2 transport design spec and implementation plan (#56)
- **re:** Write up the client's transport and handshake (#71)
- **re:** Settle the evidence norm for gujianol.exe and read AurogonEngine's exports (#73)
- **re:** Take the real client through the hello and auth (#79)
- **re:** Read what follows the client's auth: dispatch, opcodes, payloads (#80)
- **re:** Summarise M2 and update the spec (#86)

## [0.2.0] - 2026-10-12

### Added

- **data:** Read the client's blob format and its embedded file system (#40)
- **lua:** Parse and write the client's Lua 5.3 chunks (#42)
- **lua:** Disassemble the client's Lua chunks (#43)
- **tools:** Add luatool to list, extract and disassemble the embedded scripts (#44)
- **lua:** Normalise client chunks to stock Lua 5.3 and decompile them with unluac (#45)
- **lua:** Measure server-side logic in the client's scripts (#46)
- **data:** Read the archives' structure without decompression (#47)
- **re:** Track the client runner and AGT watchdog, and pin GE-Proton (#48)
- **re:** Connect the real client to fenji-stub (#49)

### Fixed

- **re:** Close the M1 final review's findings (#52)

### Documentation

- Add the M1 data-layer design spec (#22)
- Add the M1 data-layer implementation plan (#23)
- **re:** Write up the client's embedded script format (#38)
- **re:** Trace how the client finds its server (#41)
- **re:** Summarise M1 and update the spec (#50)

## [0.1.0] - 2026-10-11

### Added

- **tools:** Add fenji-stub protocol-discovery listener
- **re:** Add the loopback-only sandbox for client runs (#8)
- **re:** Add --no-root to isolated-run for tools that refuse uid 0 (#10)
- **re:** Add --cpu-topology to isolated-run for Wine's processor info (#14)

### Fixed

- Close bypasses in the forbidden-file check

### Documentation

- Add Fenji design spec
- Add M0 implementation plan
- Add docs index, development guide, RE workflow and protocol notes
- Add changelog generated by git-cliff
- **re:** Record pinned client build (#9)
- **re:** Add spike-0 static triage of the pinned client (#12)
- **re:** Add spike-0 isolated runtime observation (#15)
- **re:** Conclude spike-0 and update spec (#16)
- Apply the M0 final review fixes (#20)

<!-- generated by git-cliff -->
