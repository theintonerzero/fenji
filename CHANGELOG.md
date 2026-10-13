# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
