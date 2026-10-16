# The launcher's fixtures

The contract between the launcher's Rust model (`crates/fenji-launcher/src/app/`) and its TypeScript view (launcher spec §3.1; plan LP1, LP2, LP13). Both sides' tests read these files, so a change on either side fails the other's tests.

- `snapshots/<state>.json`: one `Snapshot` for each state a board in `docs/design/launcher/boards/` shows, built by hand in `app/golden.rs` with the boards' values (Moonward, hosted by Wen, the player lin_yue). Rust's test checks each file against its state and fails on a file no state writes. After changing a snapshot type or a state, write them again with `FENJI_BLESS=1 cargo test -p fenji-launcher golden`, and read each changed file against its board before committing.
- `intents.json`: an example of every intent the view sends, written by hand. Rust reads each one, and its test fails when an intent has no example.

The times are Unix milliseconds on the boards' clock, the evening of 2026-10-15 in Melbourne (UTC+11): `1792055640000` is 20:14.

The view's TypeScript types for both are generated from the Rust types by ts-rs into `apps/launcher/src/gen/` when `fenji-launcher`'s tests run. CI fails when the committed snapshots or generated types differ from what the tests write.
