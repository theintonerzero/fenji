# Fenji's desktop launcher

The launcher takes a player from their host's invite link to the game's lobby (launcher spec, `docs/superpowers/specs/2026-10-15-m4-launcher-design.md`): a list of servers, accounts, the player's own copy of the game checked, downloaded and repaired through DepotDownloader, and Play through umu-run.

It has three parts:

- **The model**, `crates/fenji-launcher` (`src/app/`): every piece of state and every secret. It publishes a snapshot of the state on each change, and takes intents.
- **The shell**, `src-tauri/`: a thin Tauri 2 app, a Cargo workspace of its own. It has one command for every intent and one for the current snapshot, and sends each new snapshot as the `snapshot` event. It does what only it can: the window, the folder dialog, and links (the repository's pages and each server's source code only).
- **The view**, `src/`: TypeScript that renders snapshots and sends intents (`src/ipc.ts`), with types generated from the model's (`src/gen/`). Until the screens come, it shows the snapshot as text.

`fixtures/` holds the golden snapshots and the intents' examples that both sides test against.

Building and running it: `docs/development.md`, "The desktop launcher". Its icons: `src-tauri/icons/README.md`.
