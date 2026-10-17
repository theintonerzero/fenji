# DepotDownloader 3.4.0's own output

What DepotDownloader 3.4.0 printed on the game box in the M4 follow-ups' Task 11 rehearsal (#428, 2026-10-17 UTC). It ran with `-qr` as a child, with its input closed and its output piped, the way the launcher runs it hidden (launcher spec Amendment A, LA4). `src/depot.rs`'s tests read these files through the launcher's `Reader`, and play them back through a fake DepotDownloader.

| File | The run | Its end |
|---|---|---|
| `sign-in.txt` | A repair of one file (`bin64/loader.ini`): the code, scanned with the Steam Mobile App, then the download | exit 0 |
| `expired.txt` | Nobody scans: DepotDownloader draws a new code twice, then gives up | exit 1 |
| `expired.stderr.txt` | What that run wrote on its error output | |
| `cancel.txt` | Cancelled while the code waited, with SIGTERM | killed by the signal (143 from a shell) |

The lines are DepotDownloader's own, in its order, with what identifies the account replaced:

- **The account name:** `lin_yue_on_steam`, the tests' own.
- **Every path:** a neutral `/home/player/…`. The progress line names the file by its absolute path.
- **The CellID:** `0`. It points at a region.
- **The licence count:** `12`.
- **The manifest request code:** `1234567890123456789`. It's a short-lived token.
- **The codes:** synthetic. Each is `reading_tests::code(37, n)` as DepotDownloader draws a code: 37 rows of `██` or two spaces a module, the 4-module quiet zone included, the first code `n = 1`, then 2 and 3. The real codes were Steam's sign-in challenges for the owner's account.

A real code is 29 modules on a side inside its 4-module quiet zone, 37 in all. A new code comes after an empty line and `The QR code has changed:`, under the same header. The game's depot and manifest IDs are public, and in the launcher's manual command already.
