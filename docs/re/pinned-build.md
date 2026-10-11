# Pinned client build

| Item | Value |
|---|---|
| Game | Swords of Legends Online (Gameforge's Western build of 古剑奇谭网络版) |
| Steam AppID | 1418100 (delisted 2023-06-01; servers shut down 2023-06-30) |
| Depot → manifest | 1418101 → 5002037851744157048 |
| Build ID | 10015727 |
| Main executable | `bin64/gujianol.exe`, 54,029,984 bytes, SHA-256 `3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678`, FileVersion 2.1.19.42851 (ProductVersion `42851-5993a494`) |
| Files | 210 files, 107,996,004,001 bytes: exactly the depot's `SizeOnDisk` in the Steam manifest |
| Recorded | 2026-10-11, from the owner's Steam install on the Fedora box (download finished 2026-10-11 19:32:45 AEDT) |

The pinned copy lives in `$FENJI_CLIENT_PATH` (default `~/fenji-client`). It is a reflink (copy-on-write) copy of the owner's Steam install, made outside the Steam library, so the sandbox can bind it and Steam never writes to it. See `docs/re/README.md` for how to make your own, and compare your `bin64/gujianol.exe` with the SHA-256 above.

## Run-time artefacts removed

The owner launched the game once through Steam after the download (2026-10-11 19:33 AEDT). That run left four files in the install, and all four were created by it rather than shipped. Their sizes add up to the 8,357 bytes by which the install exceeded the depot's recorded size. They were deleted from the pinned copy only; the Steam install is unchanged.

| File | Bytes | Written |
|---|---|---|
| `bin64/gujianol.log` | 3,421 | 19:33:36 |
| `bin64/Log_gujianol.htm` | 3,914 | 19:33:06 |
| `settings/GameSetting.dat` | 1,019 | 19:33:10 |
| `settings/PanelPositionSetings.dat` | 3 | 19:33:36 |

The now-empty `settings/` directory is left in place; the client writes its settings there on first run.
