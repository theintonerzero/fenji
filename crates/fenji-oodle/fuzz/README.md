# fenji-oodle's fuzz target

`decompress` hands libFuzzer's inputs to `fenji_oodle::decompress` as an archive's Oodle block holds them (`docs/re/m1-archives.md` 3.4): a little-endian `u32` raw length, here at most 512 KiB as fenji-data bounds it, then one Oodle stream. Each input must decode to an error or to exactly its raw length, and its header walk must succeed where the decode does. A panic, or an allocation past libFuzzer's limit, is a crash. `tests/robustness.rs` runs the same check on fixed seeds in CI; this target is for longer runs, locally, on nightly Rust, and never in CI.

The crate is a workspace of its own, outside Fenji's (its `[workspace]` table, and the root `Cargo.toml`'s `exclude`), so `cargo test --workspace` and `cargo deny check` don't see it or `libfuzzer-sys`, its one dependency. Its build, lockfile, corpus and crashes stay local (`.gitignore`). Like fenji-oodle, it's GPL-3.0-or-later.

## Running it

Once: `rustup toolchain install nightly` and `cargo install --locked cargo-fuzz`.

Then, from `crates/fenji-oodle`, seed the corpus with the fixtures, each behind the raw length its manifest row gives, and fuzz for five minutes:

```bash
mkdir -p fuzz/corpus/decompress
tail -n +2 tests/fixtures/manifest.tsv | while IFS=$'\t' read -r file _ size _; do
  python3 -I -c 'import sys; sys.stdout.buffer.write(int(sys.argv[1]).to_bytes(4, "little"))' "$size" |
    cat - "tests/fixtures/$file" > "fuzz/corpus/decompress/$file"
done
cargo +nightly fuzz run -a decompress -- -max_total_time=300
```

`-a` builds with debug assertions, which turn on overflow checks too. libFuzzer writes a crash to `fuzz/artifacts/decompress/`, and `cargo +nightly fuzz run -a decompress <that file>` replays it. A crash is a bug in the decoder: it becomes a case in `tests/robustness.rs`, then a fix.
