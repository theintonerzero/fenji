//! The real thing: the pinned client's embedded scripts. Local only; it needs the client copy.
use std::path::Path;

use fenji_luatool::client::{self, Decoded};

#[test]
#[ignore = "needs the pinned client: FENJI_CLIENT_PATH=~/fenji-client cargo test -p fenji-luatool --release -- --ignored"]
fn pinned_client_embedded_scripts() {
    let root = std::env::var_os("FENJI_CLIENT_PATH").expect("set FENJI_CLIENT_PATH");
    let image = client::load(&Path::new(&root).join("bin64").join("gujianol.exe"), false).unwrap();
    let fs = fenji_data::ExeFs::scan(&image).unwrap();
    // docs/re/m1-script-format.md §1 and §5: 21,512 files, every one of them decodes.
    assert_eq!(fs.entries().len(), 21_512);
    let (mut lua, mut other) = (0u32, 0u32);
    for e in fs.entries() {
        match client::decode(&fs, e) {
            Decoded::Lua(bytes, dialect) => {
                let chunk = fenji_lua::parse(&bytes, dialect)
                    .unwrap_or_else(|err| panic!("{}: {err}", e.engine_path()));
                assert!(!fenji_lua::disassemble(&chunk).is_empty());
                lua += 1;
            }
            Decoded::Other(_) => other += 1,
            Decoded::Failed(why) => panic!(
                "{} ({} bytes) doesn't decode: {why}",
                e.engine_path(),
                e.stored_size
            ),
        }
    }
    eprintln!("{lua} Lua chunks parsed and disassembled, {other} other files");
    assert_eq!((lua, other), (20_443, 1_069));
}
