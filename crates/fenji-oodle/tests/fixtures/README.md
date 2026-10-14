# fenji-oodle's test fixtures

Each `.oz` file here is one Oodle stream: Fenji's own synthetic bytes, compressed. The streams hold no game data and no Epic code.

`manifest.tsv` lists every file with:
- its input's kind, size and seed, from which `tests/gen` regenerates the input;
- the compressor and the level;
- the 64-bit FNV-1a hashes of the input and of the stream (`tests/common`), which the tests check.

## How they were made

- **When:** on 2026-10-14, with the owner's approval (the Oodle decoder spec, §4.2).
- **The DLL:** the pinned client's own `oo2core_6_win64.dll`, SHA-256 `4d7fedf085b0fc17bb124d6790047ad5228e2eb08a614a99adfdb35384c820af`.
- **The helper:** `tools/oodle-helper` at commit `b3aaa7496bd8b6a6aeea0367cd3896710feda944` (PR #243). It runs under Wine, inside the sandbox, through `scripts/spike/run-oodle-helper.sh`. It writes a stream only once the DLL decodes it back to its input.
- **The commands:**

  ```sh
  cargo run -p fenji-oodle --example write_inputs re/oodle-fixtures/in
  scripts/spike/run-oodle-helper.sh --compress re/oodle-fixtures/in re/oodle-fixtures/out 8 4
  scripts/spike/run-oodle-helper.sh --compress re/oodle-fixtures/in re/oodle-fixtures/out 8 8
  ```

  Then `re/oodle-fixtures/out/*.oz` was copied here. Compressor 8 is Kraken, the only Oodle codec in the pinned client (`docs/re/m1-archives.md` 3.6).
- **Deterministic:** the 22 fixtures first made at commit `8708573` came out the same, byte for byte, at `b3aaa74`.

To add a fixture, add its case to `CASES` in `tests/gen`, make and copy its streams as above, and add its rows. While a row's hash is wrong, the hash test fails and prints the hash it computed.
