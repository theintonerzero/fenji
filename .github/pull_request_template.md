Closes #

## What and why

<!-- One concern per PR. If you're describing two unrelated changes, split the PR. -->

## How it was verified

<!-- Commands run and their results; for client-facing changes, the manual Proton run. -->

## Checklist

- [ ] PR title is a Conventional Commit (`type(scope): description`); it becomes the squash commit
- [ ] Targets `dev` (or `main` only for `release/*` / `hotfix/*`)
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass
- [ ] `scripts/check-forbidden-files.sh` passes; no game files, captures, decompiled code or credentials
- [ ] Protocol facts cite REA Evidence IDs; docs updated where behaviour changed
